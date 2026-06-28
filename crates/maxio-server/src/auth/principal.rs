/// Authenticated S3 access key attached to a request after SigV4 verification.
#[derive(Clone, Debug)]
pub struct AuthPrincipal {
    pub access_key: String,
}
