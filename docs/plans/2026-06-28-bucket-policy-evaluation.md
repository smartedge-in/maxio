# Bucket policy evaluation (P1-11)

## Goal

Accept S3-style JSON bucket policies for public access patterns while documenting a deliberate v1 subset.

## MinIO / AWS comparison

MinIO implements a large fraction of AWS IAM policy grammar (statements, principals, conditions, Deny, etc.). MaxIO v1 intentionally implements only the smallest cross-section needed for **anonymous read/list** compatible with static website and public asset buckets.

## v1 supported subset

| Field | v1 rule |
|-------|---------|
| `Version` | `2012-10-17` or omitted |
| `Statement[].Effect` | `Allow` only |
| `Statement[].Principal` | `"*"` or `{"AWS":"*"}` |
| `Statement[].Action` | `s3:GetObject` and/or `s3:ListBucket` |
| `Statement[].Resource` | `arn:aws:s3:::bucket` for ListBucket; `arn:aws:s3:::bucket/*` for GetObject |

**API:** `PUT/GET/DELETE ?policy` on the bucket (same as S3).

**Effect:** Parsed policy updates `public_read` / `public_list` on bucket metadata (same flags the console toggles). Policy JSON is stored in `.bucket.json` as `bucket_policy`.

**Auth bypass:** Anonymous `GET`/`HEAD`/`OPTIONS` on objects when `public_read`; list calls when `public_list`. Mutating sub-resources (`?policy`, `?acl`, `?uploads`, etc.) never bypass auth.

## Explicit non-goals (v1)

- `Deny` statements and explicit deny-wins evaluation
- Per-principal ARNs (IAM users, accounts, roles)
- `Condition` blocks (IP, TLS, prefix, etc.)
- Actions beyond `GetObject` / `ListBucket` (PutObject, DeleteObject, multipart, etc.)
- Bucket policy + object ACL composition
- Policy versioning and `PolicyStatus` API
- Cross-bucket or `*` resource ARNs (except `*` matching the bucket ARN check)
- IAM policy simulation or `EvaluatePolicy` tooling

## Error behaviour

- Malformed JSON or unsupported statements → `MalformedPolicy` (HTTP 400).
- `GET ?policy` when none set → `NoSuchBucketPolicy` (HTTP 404).

## Future work

1. Principal-specific Allow rules mapped to access keys.
2. Condition keys for prefix and source IP.
3. Deny statements with AWS deny-overrides-allow ordering.
4. Separate policy document from console public toggles (evaluate on each request).