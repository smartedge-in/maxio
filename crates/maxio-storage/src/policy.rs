//! S3 bucket policy parser and evaluator (v1 public flags + v2 request evaluation).
//!
//! See `docs/plans/2026-06-28-bucket-policy-evaluation.md` for v1 subset and P3-28 v2 extensions.

use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::net::IpAddr;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PolicyEffects {
    pub public_read: bool,
    pub public_list: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny,
    ImplicitDeny,
}

/// Request context for v2 policy evaluation.
#[derive(Debug, Clone, Default)]
pub struct PolicyContext {
    pub principal_access_key: Option<String>,
    pub source_ip: Option<String>,
    pub is_anonymous: bool,
    /// JWT `groups` claim values (Keycloak / OIDC), when available.
    pub jwt_groups: Vec<String>,
    /// JWT `roles` claim values (Keycloak realm/client roles), when available.
    pub jwt_roles: Vec<String>,
    /// Optional raw OIDC claim map for future condition keys.
    pub oidc_claims: Option<HashMap<String, Value>>,
}

/// Target S3 operation for v2 evaluation.
#[derive(Debug, Clone)]
pub struct PolicyRequest {
    pub action: String,
    pub bucket: String,
    pub object_key: Option<String>,
    pub list_prefix: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BucketPolicy {
    #[serde(default)]
    version: String,
    statement: Vec<PolicyStatement>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PolicyStatement {
    effect: String,
    principal: Value,
    action: PolicyActions,
    resource: PolicyResources,
    #[serde(default)]
    condition: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PolicyActions {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PolicyResources {
    One(String),
    Many(Vec<String>),
}

const SUPPORTED_ACTIONS: &[&str] = &[
    "s3:GetObject",
    "s3:PutObject",
    "s3:DeleteObject",
    "s3:ListBucket",
    "s3:*",
];

const CONDITION_OPERATORS: &[&str] = &["StringEquals", "StringLike", "IpAddress"];

/// Parse and evaluate the v1-supported bucket policy subset.
///
/// Supported: `Effect: Allow`, `Principal: "*"`, actions `s3:GetObject` and/or
/// `s3:ListBucket` scoped to the target bucket ARN. Everything else is rejected.
pub fn evaluate_v1_policy(bucket: &str, raw: &str) -> Result<PolicyEffects, String> {
    let policy: BucketPolicy =
        serde_json::from_str(raw).map_err(|e| format!("MalformedPolicy: invalid JSON: {e}"))?;

    if !policy.version.is_empty() && policy.version != "2012-10-17" {
        return Err(format!(
            "MalformedPolicy: unsupported Version '{}', expected 2012-10-17",
            policy.version
        ));
    }

    let mut effects = PolicyEffects::default();
    let bucket_arn = format!("arn:aws:s3:::{bucket}");
    let object_arn = format!("arn:aws:s3:::{bucket}/*");

    for stmt in &policy.statement {
        if stmt.condition.is_some() {
            return Err("MalformedPolicy: Condition blocks are not supported in v1".into());
        }
        if !stmt.effect.eq_ignore_ascii_case("Allow") {
            return Err("MalformedPolicy: only Effect=Allow statements are supported in v1".into());
        }
        if !principal_is_wildcard(&stmt.principal) {
            return Err("MalformedPolicy: only Principal=\"*\" is supported in v1".into());
        }

        let mut apply_action = |raw_action: &str| -> Result<(), String> {
            let action = normalize_action(raw_action);
            match action.as_str() {
                "s3:GetObject" => {
                    if any_resource(&stmt.resource, &object_arn) {
                        effects.public_read = true;
                        Ok(())
                    } else {
                        Err(format!(
                            "MalformedPolicy: s3:GetObject requires Resource {object_arn}"
                        ))
                    }
                }
                "s3:ListBucket" => {
                    if any_resource(&stmt.resource, &bucket_arn) {
                        effects.public_list = true;
                        Ok(())
                    } else {
                        Err(format!(
                            "MalformedPolicy: s3:ListBucket requires Resource {bucket_arn}"
                        ))
                    }
                }
                other => Err(format!(
                    "MalformedPolicy: unsupported Action '{other}' in v1 (allowed: s3:GetObject, s3:ListBucket)"
                )),
            }
        };

        match &stmt.action {
            PolicyActions::One(s) => apply_action(s)?,
            PolicyActions::Many(v) => {
                for s in v {
                    apply_action(s)?;
                }
            }
        }
    }

    Ok(effects)
}

/// True when authenticated requests must be evaluated against v2 rules.
///
/// Pure v1 policies (wildcard Allow for GetObject/ListBucket only) continue to
/// gate anonymous access via `public_read` / `public_list` without restricting
/// authenticated SigV4 callers.
pub fn policy_requires_v2_enforcement(bucket: &str, raw: &str) -> bool {
    evaluate_v1_policy(bucket, raw).is_err()
}

/// Validate a v2 bucket policy document for the given bucket (structure and scope).
pub fn validate_policy_v2(bucket: &str, raw: &str) -> Result<(), String> {
    let policy = parse_policy(raw)?;
    let bucket_arn = format!("arn:aws:s3:::{bucket}");
    let object_arn_prefix = format!("arn:aws:s3:::{bucket}/");

    if policy.statement.is_empty() {
        return Err("MalformedPolicy: Statement must not be empty".into());
    }

    for stmt in &policy.statement {
        validate_effect(&stmt.effect)?;
        validate_principal(&stmt.principal)?;
        validate_actions(&stmt.action)?;
        validate_resources_for_bucket(&stmt.resource, &bucket_arn, &object_arn_prefix)?;
        if let Some(cond) = &stmt.condition {
            validate_condition(cond)?;
        }
    }

    Ok(())
}

/// Evaluate v2 policy for a concrete request. Deny statements win over Allow.
pub fn evaluate_policy_v2(
    raw: &str,
    req: &PolicyRequest,
    ctx: &PolicyContext,
) -> Result<PolicyDecision, String> {
    let policy = parse_policy(raw)?;
    let bucket_arn = format!("arn:aws:s3:::{}", req.bucket);
    let object_arn = req
        .object_key
        .as_ref()
        .map(|key| format!("arn:aws:s3:::{}/{}", req.bucket, key))
        .unwrap_or_else(|| format!("arn:aws:s3:::{}/{}", req.bucket, "*"));

    let mut allow_match = false;
    let mut deny_match = false;

    for stmt in &policy.statement {
        if !statement_matches_request(stmt, req, &bucket_arn, &object_arn, ctx) {
            continue;
        }
        if stmt.effect.eq_ignore_ascii_case("Deny") {
            deny_match = true;
        } else if stmt.effect.eq_ignore_ascii_case("Allow") {
            allow_match = true;
        } else {
            return Err(format!(
                "MalformedPolicy: unsupported Effect '{}'",
                stmt.effect
            ));
        }
    }

    Ok(if deny_match {
        PolicyDecision::Deny
    } else if allow_match {
        PolicyDecision::Allow
    } else {
        PolicyDecision::ImplicitDeny
    })
}

fn parse_policy(raw: &str) -> Result<BucketPolicy, String> {
    let policy: BucketPolicy =
        serde_json::from_str(raw).map_err(|e| format!("MalformedPolicy: invalid JSON: {e}"))?;

    if !policy.version.is_empty() && policy.version != "2012-10-17" {
        return Err(format!(
            "MalformedPolicy: unsupported Version '{}', expected 2012-10-17",
            policy.version
        ));
    }

    Ok(policy)
}

fn statement_matches_request(
    stmt: &PolicyStatement,
    req: &PolicyRequest,
    bucket_arn: &str,
    object_arn: &str,
    ctx: &PolicyContext,
) -> bool {
    if !principal_matches(&stmt.principal, ctx) {
        return false;
    }
    if !actions_match(&stmt.action, &req.action) {
        return false;
    }
    if !resource_matches_request(&stmt.resource, req, bucket_arn, object_arn) {
        return false;
    }
    if let Some(cond) = &stmt.condition {
        if !conditions_match(cond, req, ctx) {
            return false;
        }
    }
    true
}

fn validate_effect(effect: &str) -> Result<(), String> {
    if effect.eq_ignore_ascii_case("Allow") || effect.eq_ignore_ascii_case("Deny") {
        Ok(())
    } else {
        Err(format!(
            "MalformedPolicy: unsupported Effect '{effect}' (allowed: Allow, Deny)"
        ))
    }
}

fn validate_principal(principal: &Value) -> Result<(), String> {
    if principal_is_wildcard(principal) {
        return Ok(());
    }
    for arn in principal_arns(principal) {
        if access_key_from_principal_arn(&arn).is_none() {
            return Err(format!(
                "MalformedPolicy: unsupported Principal ARN '{arn}' (expected arn:aws:iam:::user/ACCESS_KEY)"
            ));
        }
    }
    Ok(())
}

fn validate_actions(actions: &PolicyActions) -> Result<(), String> {
    let list = actions_list(actions);
    if list.is_empty() {
        return Err("MalformedPolicy: Action must not be empty".into());
    }
    for action in list {
        let normalized = normalize_action(&action);
        if !SUPPORTED_ACTIONS.contains(&normalized.as_str()) {
            return Err(format!(
                "MalformedPolicy: unsupported Action '{action}' (allowed: s3:GetObject, s3:PutObject, s3:DeleteObject, s3:ListBucket, s3:*)"
            ));
        }
    }
    Ok(())
}

fn validate_resources_for_bucket(
    resources: &PolicyResources,
    bucket_arn: &str,
    object_arn_prefix: &str,
) -> Result<(), String> {
    let list = resources_list(resources);
    if list.is_empty() {
        return Err("MalformedPolicy: Resource must not be empty".into());
    }
    for resource in list {
        if resource == "*"
            || resource == bucket_arn
            || resource.starts_with(object_arn_prefix)
            || wildcard_resource_matches_bucket(&resource, bucket_arn, object_arn_prefix)
        {
            continue;
        }
        return Err(format!(
            "MalformedPolicy: Resource '{resource}' must scope to bucket ARN {bucket_arn} or {object_arn_prefix}*"
        ));
    }
    Ok(())
}

fn wildcard_resource_matches_bucket(resource: &str, bucket_arn: &str, object_prefix: &str) -> bool {
    if !resource.contains('*') {
        return false;
    }
    glob_match(resource, bucket_arn) || glob_match(resource, &format!("{object_prefix}example-key"))
}

fn validate_condition(condition: &Value) -> Result<(), String> {
    let Some(obj) = condition.as_object() else {
        return Err("MalformedPolicy: Condition must be a JSON object".into());
    };
    if obj.is_empty() {
        return Err("MalformedPolicy: Condition must not be empty".into());
    }
    for (operator, body) in obj {
        if !CONDITION_OPERATORS
            .iter()
            .any(|op| op.eq_ignore_ascii_case(operator))
        {
            return Err(format!(
                "MalformedPolicy: unsupported Condition operator '{operator}' (allowed: StringEquals, StringLike, IpAddress)"
            ));
        }
        let Some(keys) = body.as_object() else {
            return Err(format!(
                "MalformedPolicy: Condition operator '{operator}' must map condition keys to values"
            ));
        };
        for key in keys.keys() {
            if !is_supported_condition_key(key) {
                return Err(format!(
                    "MalformedPolicy: unsupported Condition key '{key}'"
                ));
            }
        }
    }
    Ok(())
}

fn is_supported_condition_key(key: &str) -> bool {
    matches!(
        key,
        "aws:SourceIp" | "s3:prefix" | "jwt:groups" | "jwt:roles"
    )
}

fn principal_matches(principal: &Value, ctx: &PolicyContext) -> bool {
    if principal_is_wildcard(principal) {
        return true;
    }
    let Some(access_key) = ctx.principal_access_key.as_deref() else {
        return false;
    };
    principal_arns(principal)
        .any(|arn| access_key_from_principal_arn(&arn).is_some_and(|k| k == access_key))
}

fn principal_arns(principal: &Value) -> impl Iterator<Item = String> + '_ {
    let mut arns: Vec<String> = Vec::new();
    match principal {
        Value::String(s) if s != "*" => arns.push(s.clone()),
        Value::Object(map) => {
            if let Some(Value::String(s)) = map.get("AWS") {
                if s != "*" {
                    arns.push(s.clone());
                }
            } else if let Some(Value::Array(arr)) = map.get("AWS") {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        if s != "*" {
                            arns.push(s.to_string());
                        }
                    }
                }
            }
        }
        _ => {}
    }
    arns.into_iter()
}

fn access_key_from_principal_arn(arn: &str) -> Option<&str> {
    let (_, suffix) = arn.rsplit_once(":user/")?;
    if suffix.is_empty() {
        None
    } else {
        Some(suffix)
    }
}

fn actions_match(actions: &PolicyActions, requested: &str) -> bool {
    let requested = normalize_action(requested);
    actions_list(actions).iter().any(|raw| {
        let action = normalize_action(raw);
        action == "s3:*" || action == requested
    })
}

fn actions_list(actions: &PolicyActions) -> Vec<String> {
    match actions {
        PolicyActions::One(s) => vec![s.clone()],
        PolicyActions::Many(v) => v.clone(),
    }
}

fn resource_matches_request(
    resources: &PolicyResources,
    req: &PolicyRequest,
    bucket_arn: &str,
    object_arn: &str,
) -> bool {
    let target = if req.action == "s3:ListBucket" {
        bucket_arn
    } else {
        object_arn
    };
    resources_list(resources)
        .iter()
        .any(|pattern| resource_pattern_matches(pattern, target, bucket_arn))
}

fn resources_list(resources: &PolicyResources) -> Vec<String> {
    match resources {
        PolicyResources::One(s) => vec![s.clone()],
        PolicyResources::Many(v) => v.clone(),
    }
}

fn resource_pattern_matches(pattern: &str, target: &str, bucket_arn: &str) -> bool {
    if pattern == "*" || pattern == target {
        return true;
    }
    if pattern.contains('*') || pattern.contains('?') {
        return glob_match(pattern, target);
    }
    pattern == bucket_arn && target.starts_with(bucket_arn)
}

fn conditions_match(condition: &Value, req: &PolicyRequest, ctx: &PolicyContext) -> bool {
    let Some(obj) = condition.as_object() else {
        return false;
    };
    for (operator, body) in obj {
        let Some(keys) = body.as_object() else {
            return false;
        };
        for (key, expected) in keys {
            if !condition_key_matches(operator, key, expected, req, ctx) {
                return false;
            }
        }
    }
    true
}

fn condition_key_matches(
    operator: &str,
    key: &str,
    expected: &Value,
    req: &PolicyRequest,
    ctx: &PolicyContext,
) -> bool {
    match key {
        "aws:SourceIp" if operator.eq_ignore_ascii_case("IpAddress") => {
            let Some(ip) = ctx.source_ip.as_deref() else {
                return false;
            };
            condition_values(expected)
                .iter()
                .any(|pattern| ip_matches_cidr(ip, pattern))
        }
        "s3:prefix" => {
            let prefix = req
                .list_prefix
                .as_deref()
                .or(req.object_key.as_deref())
                .unwrap_or("");
            condition_values(expected).iter().any(|pattern| {
                if operator.eq_ignore_ascii_case("StringEquals") {
                    prefix == pattern
                } else if operator.eq_ignore_ascii_case("StringLike") {
                    glob_match(pattern, prefix)
                } else {
                    false
                }
            })
        }
        "jwt:groups" => jwt_claim_condition_matches(operator, expected, &ctx.jwt_groups),
        "jwt:roles" => jwt_claim_condition_matches(operator, expected, &ctx.jwt_roles),
        _ => false,
    }
}

fn jwt_claim_condition_matches(operator: &str, expected: &Value, actual: &[String]) -> bool {
    condition_values(expected).iter().any(|pattern| {
        actual.iter().any(|value| {
            if operator.eq_ignore_ascii_case("StringEquals") {
                value == pattern
            } else if operator.eq_ignore_ascii_case("StringLike") {
                glob_match(pattern, value)
            } else {
                false
            }
        })
    })
}

fn condition_values(value: &Value) -> Vec<String> {
    match value {
        Value::String(s) => vec![s.clone()],
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn ip_matches_cidr(ip: &str, cidr: &str) -> bool {
    let Ok(addr) = ip.parse::<IpAddr>() else {
        return false;
    };
    if let Some((network, prefix)) = cidr.split_once('/') {
        let Ok(net_addr) = network.parse::<IpAddr>() else {
            return false;
        };
        let Ok(prefix_len) = prefix.parse::<u8>() else {
            return false;
        };
        ip_in_network(addr, net_addr, prefix_len)
    } else {
        ip == cidr
    }
}

fn ip_in_network(ip: IpAddr, network: IpAddr, prefix_len: u8) -> bool {
    match (network, ip) {
        (IpAddr::V4(net), IpAddr::V4(addr)) => {
            let mask = if prefix_len >= 32 {
                !0u32
            } else if prefix_len == 0 {
                0
            } else {
                !0u32 << (32 - prefix_len)
            };
            let net_bits = u32::from_be_bytes(net.octets());
            let addr_bits = u32::from_be_bytes(addr.octets());
            (net_bits & mask) == (addr_bits & mask)
        }
        (IpAddr::V6(net), IpAddr::V6(addr)) => {
            let net_segments = net.octets();
            let addr_segments = addr.octets();
            let full_bytes = (prefix_len / 8) as usize;
            let remainder = prefix_len % 8;
            if net_segments[..full_bytes] != addr_segments[..full_bytes] {
                return false;
            }
            if remainder == 0 {
                return true;
            }
            let mask = 0xff << (8 - remainder);
            (net_segments[full_bytes] & mask) == (addr_segments[full_bytes] & mask)
        }
        _ => false,
    }
}

fn glob_match(pattern: &str, value: &str) -> bool {
    let p: Vec<_> = pattern.chars().collect();
    let v: Vec<_> = value.chars().collect();
    let mut pi = 0usize;
    let mut vi = 0usize;
    let mut star_pi = None;
    let mut star_vi = None;

    while vi < v.len() {
        if pi < p.len() && (p[pi] == v[vi] || p[pi] == '?') {
            pi += 1;
            vi += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_pi = Some(pi);
            star_vi = Some(vi);
            pi += 1;
        } else if let (Some(spi), Some(svi)) = (star_pi, star_vi) {
            pi = spi + 1;
            vi = svi + 1;
            star_vi = Some(svi + 1);
        } else {
            return false;
        }
    }

    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn principal_is_wildcard(principal: &Value) -> bool {
    match principal {
        Value::String(s) => s == "*",
        Value::Object(map) => map.get("AWS").is_some_and(|v| match v {
            Value::String(s) => s == "*",
            Value::Array(arr) => arr.iter().any(|x| x.as_str() == Some("*")),
            _ => false,
        }),
        _ => false,
    }
}

fn normalize_action(action: &str) -> String {
    if let Some((service, rest)) = action.split_once(':')
        && service.eq_ignore_ascii_case("s3")
    {
        return format!("s3:{rest}");
    }
    action.to_string()
}

fn any_resource(resources: &PolicyResources, expected: &str) -> bool {
    match resources {
        PolicyResources::One(s) => resource_matches(s, expected),
        PolicyResources::Many(v) => v.iter().any(|s| resource_matches(s, expected)),
    }
}

fn resource_matches(resource: &str, expected: &str) -> bool {
    resource == expected || resource == "*"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_policy(bucket: &str, json: &str) -> PolicyEffects {
        evaluate_v1_policy(bucket, json).expect("policy should parse")
    }

    fn ctx_for(access_key: &str) -> PolicyContext {
        PolicyContext {
            principal_access_key: Some(access_key.to_string()),
            source_ip: Some("203.0.113.10".into()),
            is_anonymous: false,
            jwt_groups: vec!["storage-admins".into()],
            jwt_roles: vec!["maxio-write".into()],
            oidc_claims: None,
        }
    }

    fn put_req(bucket: &str, key: &str) -> PolicyRequest {
        PolicyRequest {
            action: "s3:PutObject".into(),
            bucket: bucket.into(),
            object_key: Some(key.into()),
            list_prefix: None,
        }
    }

    #[test]
    fn parses_public_read_policy() {
        let effects = read_policy(
            "photos",
            r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:GetObject",
                "Resource": "arn:aws:s3:::photos/*"
            }]
        }"#,
        );
        assert_eq!(
            effects,
            PolicyEffects {
                public_read: true,
                public_list: false
            }
        );
    }

    #[test]
    fn parses_public_list_policy() {
        let effects = read_policy(
            "photos",
            r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:ListBucket",
                "Resource": "arn:aws:s3:::photos"
            }]
        }"#,
        );
        assert_eq!(
            effects,
            PolicyEffects {
                public_read: false,
                public_list: true
            }
        );
    }

    #[test]
    fn parses_combined_read_and_list() {
        let effects = read_policy(
            "data",
            r#"{
            "Statement": [
              {
                "Effect": "Allow",
                "Principal": {"AWS": "*"},
                "Action": ["s3:GetObject", "s3:ListBucket"],
                "Resource": ["arn:aws:s3:::data/*", "arn:aws:s3:::data"]
              }
            ]
        }"#,
        );
        assert_eq!(
            effects,
            PolicyEffects {
                public_read: true,
                public_list: true
            }
        );
    }

    #[test]
    fn rejects_deny_statements_in_v1() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Deny",
                "Principal": "*",
                "Action": "s3:GetObject",
                "Resource": "arn:aws:s3:::photos/*"
            }]
        }"#;
        assert!(evaluate_v1_policy("photos", raw).is_err());
    }

    #[test]
    fn rejects_v1_when_condition_present() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:GetObject",
                "Resource": "arn:aws:s3:::photos/*",
                "Condition": {"StringEquals": {"s3:prefix": "public/"}}
            }]
        }"#;
        assert!(evaluate_v1_policy("photos", raw).is_err());
    }

    #[test]
    fn rejects_non_wildcard_principal() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Allow",
                "Principal": {"AWS": "arn:aws:iam::123:root"},
                "Action": "s3:GetObject",
                "Resource": "arn:aws:s3:::photos/*"
            }]
        }"#;
        let err = evaluate_v1_policy("photos", raw).unwrap_err();
        assert!(err.contains("Principal"));
    }

    #[test]
    fn rejects_wrong_resource_for_get_object() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:GetObject",
                "Resource": "arn:aws:s3:::photos"
            }]
        }"#;
        let err = evaluate_v1_policy("photos", raw).unwrap_err();
        assert!(err.contains("s3:GetObject"));
    }

    #[test]
    fn rejects_unsupported_action() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:PutObject",
                "Resource": "arn:aws:s3:::photos/*"
            }]
        }"#;
        let err = evaluate_v1_policy("photos", raw).unwrap_err();
        assert!(err.contains("PutObject"));
    }

    #[test]
    fn rejects_bad_version() {
        let raw = r#"{
            "Version": "2008-10-17",
            "Statement": []
        }"#;
        let err = evaluate_v1_policy("b", raw).unwrap_err();
        assert!(err.contains("Version"));
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(evaluate_v1_policy("b", "{").is_err());
    }

    #[test]
    fn rejects_wrong_resource_for_list_bucket() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:ListBucket",
                "Resource": "arn:aws:s3:::photos/*"
            }]
        }"#;
        let err = evaluate_v1_policy("photos", raw).unwrap_err();
        assert!(err.contains("s3:ListBucket"));
    }

    #[test]
    fn normalizes_s3_action_prefix_case() {
        let effects = read_policy(
            "x",
            r#"{
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "S3:GetObject",
                "Resource": "arn:aws:s3:::x/*"
            }]
        }"#,
        );
        assert!(effects.public_read);
    }

    #[test]
    fn v2_validates_deny_policy() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Deny",
                "Principal": "*",
                "Action": "s3:PutObject",
                "Resource": "arn:aws:s3:::photos/*"
            }]
        }"#;
        assert!(validate_policy_v2("photos", raw).is_ok());
    }

    #[test]
    fn v2_validates_integration_deny_get_object_policy() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Deny",
                "Principal": {"AWS": "arn:aws:iam::maxio:user/maxioadmin"},
                "Action": "s3:GetObject",
                "Resource": "arn:aws:s3:::deny-bucket/secret/*"
            }]
        }"#;
        validate_policy_v2("deny-bucket", raw).expect("integration policy must validate");
    }

    #[test]
    fn v2_deny_wins_over_allow() {
        let raw = r#"{
            "Statement": [
              {
                "Effect": "Allow",
                "Principal": {"AWS": "arn:aws:iam:::user/alice"},
                "Action": "s3:PutObject",
                "Resource": "arn:aws:s3:::photos/*"
              },
              {
                "Effect": "Deny",
                "Principal": "*",
                "Action": "s3:PutObject",
                "Resource": "arn:aws:s3:::photos/secret/*"
              }
            ]
        }"#;
        let req = put_req("photos", "secret/key.txt");
        let ctx = ctx_for("alice");
        assert_eq!(
            evaluate_policy_v2(raw, &req, &ctx).unwrap(),
            PolicyDecision::Deny
        );

        let req = put_req("photos", "public/key.txt");
        assert_eq!(
            evaluate_policy_v2(raw, &req, &ctx).unwrap(),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn v2_implicit_deny_without_matching_allow() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Allow",
                "Principal": {"AWS": "arn:aws:iam:::user/alice"},
                "Action": "s3:GetObject",
                "Resource": "arn:aws:s3:::photos/*"
            }]
        }"#;
        let req = put_req("photos", "a.txt");
        let ctx = ctx_for("alice");
        assert_eq!(
            evaluate_policy_v2(raw, &req, &ctx).unwrap(),
            PolicyDecision::ImplicitDeny
        );
    }

    #[test]
    fn v2_principal_access_key_arn() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Allow",
                "Principal": {"AWS": "arn:aws:iam:::user/bob"},
                "Action": "s3:PutObject",
                "Resource": "arn:aws:s3:::data/*"
            }]
        }"#;
        let req = put_req("data", "file.bin");
        assert_eq!(
            evaluate_policy_v2(raw, &req, &ctx_for("bob")).unwrap(),
            PolicyDecision::Allow
        );
        assert_eq!(
            evaluate_policy_v2(raw, &req, &ctx_for("carol")).unwrap(),
            PolicyDecision::ImplicitDeny
        );
    }

    #[test]
    fn v2_s3_star_action_matches_put() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:*",
                "Resource": "arn:aws:s3:::wide/*"
            }]
        }"#;
        let req = put_req("wide", "x");
        let ctx = PolicyContext::default();
        assert_eq!(
            evaluate_policy_v2(raw, &req, &ctx).unwrap(),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn v2_stringlike_prefix_condition() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Allow",
                "Principal": {"AWS": "arn:aws:iam:::user/alice"},
                "Action": "s3:PutObject",
                "Resource": "arn:aws:s3:::photos/*",
                "Condition": {"StringLike": {"s3:prefix": "public/*"}}
            }]
        }"#;
        let ctx = ctx_for("alice");
        assert_eq!(
            evaluate_policy_v2(raw, &put_req("photos", "public/a.txt"), &ctx).unwrap(),
            PolicyDecision::Allow
        );
        assert_eq!(
            evaluate_policy_v2(raw, &put_req("photos", "private/a.txt"), &ctx).unwrap(),
            PolicyDecision::ImplicitDeny
        );
    }

    #[test]
    fn v2_ip_address_condition() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:PutObject",
                "Resource": "arn:aws:s3:::net/*",
                "Condition": {"IpAddress": {"aws:SourceIp": "203.0.113.0/24"}}
            }]
        }"#;
        let mut ctx = PolicyContext::default();
        ctx.source_ip = Some("203.0.113.50".into());
        assert_eq!(
            evaluate_policy_v2(raw, &put_req("net", "a"), &ctx).unwrap(),
            PolicyDecision::Allow
        );
        ctx.source_ip = Some("198.51.100.1".into());
        assert_eq!(
            evaluate_policy_v2(raw, &put_req("net", "a"), &ctx).unwrap(),
            PolicyDecision::ImplicitDeny
        );
    }

    #[test]
    fn v2_jwt_groups_string_equals() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:PutObject",
                "Resource": "arn:aws:s3:::oidc/*",
                "Condition": {"StringEquals": {"jwt:groups": "storage-admins"}}
            }]
        }"#;
        assert_eq!(
            evaluate_policy_v2(raw, &put_req("oidc", "a"), &ctx_for("any")).unwrap(),
            PolicyDecision::Allow
        );

        let mut ctx = ctx_for("any");
        ctx.jwt_groups.clear();
        assert_eq!(
            evaluate_policy_v2(raw, &put_req("oidc", "a"), &ctx).unwrap(),
            PolicyDecision::ImplicitDeny
        );
    }

    #[test]
    fn v2_jwt_roles_string_like() {
        let raw = r#"{
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:DeleteObject",
                "Resource": "arn:aws:s3:::oidc/*",
                "Condition": {"StringLike": {"jwt:roles": "maxio-*"}}
            }]
        }"#;
        let req = PolicyRequest {
            action: "s3:DeleteObject".into(),
            bucket: "oidc".into(),
            object_key: Some("x".into()),
            list_prefix: None,
        };
        assert_eq!(
            evaluate_policy_v2(raw, &req, &ctx_for("u")).unwrap(),
            PolicyDecision::Allow
        );

        let mut ctx = ctx_for("u");
        ctx.jwt_roles = vec!["readonly".into()];
        assert_eq!(
            evaluate_policy_v2(raw, &req, &ctx).unwrap(),
            PolicyDecision::ImplicitDeny
        );
    }

    #[test]
    fn glob_match_examples() {
        assert!(glob_match("public/*", "public/a.txt"));
        assert!(!glob_match("public/*", "private/a.txt"));
        assert!(glob_match("maxio-*", "maxio-write"));
    }
}
