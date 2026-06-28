//! Minimal S3 bucket policy subset (P1-11 v1).

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Default)]
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

impl PolicyActions {
    fn iter(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            Self::One(s) => Box::new(std::iter::once(s.as_str())),
            Self::Many(v) => Box::new(v.iter().map(String::as_str)),
        }
    }
}

impl PolicyResources {
    fn iter(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            Self::One(s) => Box::new(std::iter::once(s.as_str())),
            Self::Many(v) => Box::new(v.iter().map(String::as_str)),
        }
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

        for action in stmt.action.iter() {
            let action = normalize_action(action);
            match action.as_str() {
                "s3:GetObject" => {
                    if stmt
                        .resource
                        .iter()
                        .any(|r| resource_matches(r, &object_arn))
                    {
                        effects.public_read = true;
                    } else {
                        return Err(format!(
                            "MalformedPolicy: s3:GetObject requires Resource {object_arn}"
                        ));
                    }
                }
                "s3:ListBucket" => {
                    if stmt
                        .resource
                        .iter()
                        .any(|r| resource_matches(r, &bucket_arn))
                    {
                        effects.public_list = true;
                    } else {
                        return Err(format!(
                            "MalformedPolicy: s3:ListBucket requires Resource {bucket_arn}"
                        ));
                    }
                }
                other => {
                    return Err(format!(
                        "MalformedPolicy: unsupported Action '{other}' in v1 (allowed: s3:GetObject, s3:ListBucket)"
                    ));
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
    if let Some((_service, rest)) = action.split_once(':') {
        if _service == "s3" || _service.eq_ignore_ascii_case("s3") {
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

    #[test]
    fn parses_public_read_policy() {
        let raw = r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:GetObject",
                "Resource": "arn:aws:s3:::photos/*"
            }]
        }"#;
        let effects = evaluate_v1_policy("photos", raw).unwrap();
        assert!(effects.public_read);
        assert!(!effects.public_list);
    }

    #[test]
    fn parses_public_list_policy() {
        let raw = r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:ListBucket",
                "Resource": "arn:aws:s3:::photos"
            }]
        }"#;
        let effects = evaluate_v1_policy("photos", raw).unwrap();
        assert!(!effects.public_read);
        assert!(effects.public_list);
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
}