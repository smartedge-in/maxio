//! Minimal S3 bucket policy subset (P1-11 v1).
//!
//! See `docs/plans/2026-06-28-bucket-policy-evaluation.md` for supported grammar and non-goals.

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PolicyEffects {
    pub public_read: bool,
    pub public_list: bool,
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

fn any_resource(resources: &PolicyResources, expected: &str) -> bool {
    match resources {
        PolicyResources::One(s) => resource_matches(s, expected),
        PolicyResources::Many(v) => v.iter().any(|s| resource_matches(s, expected)),
    }
}

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
        if !stmt.effect.eq_ignore_ascii_case("Allow") {
            return Err(
                "MalformedPolicy: only Effect=Allow statements are supported in v1".into(),
            );
        }
        if !principal_is_wildcard(&stmt.principal) {
            return Err(
                "MalformedPolicy: only Principal=\"*\" is supported in v1".into(),
            );
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
    if let Some((service, rest)) = action.split_once(':') {
        if service.eq_ignore_ascii_case("s3") {
            return format!("s3:{rest}");
        }
    }
    action.to_string()
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
    fn rejects_deny_statements() {
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
}