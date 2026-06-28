use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use http::StatusCode;

#[derive(Debug)]
pub struct S3Error {
    pub code: S3ErrorCode,
    pub message: String,
    pub resource: Option<String>,
    pub retry_after_secs: Option<u64>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum S3ErrorCode {
    AccessDenied,
    BadDigest,
    BucketAlreadyOwnedByYou,
    BucketNotEmpty,
    InternalError,
    InvalidAccessKeyId,
    InvalidArgument,
    InvalidBucketName,
    InvalidPart,
    MalformedXML,
    NoSuchBucket,
    NoSuchKey,
    NoSuchUpload,
    NoSuchVersion,
    InvalidRange,
    NotImplemented,
    EntityTooSmall,
    EntityTooLarge,
    InsufficientStorage,
    ExpiredPresignedUrl,
    NoSuchCORSConfiguration,
    NoSuchBucketPolicy,
    MalformedPolicy,
    PreconditionFailed,
    SignatureDoesNotMatch,
    InvalidEncryptionAlgorithm,
    ServerSideEncryptionConfigurationNotFound,
    SlowDown,
}

impl S3ErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AccessDenied => "AccessDenied",
            Self::BadDigest => "BadDigest",
            Self::BucketAlreadyOwnedByYou => "BucketAlreadyOwnedByYou",
            Self::BucketNotEmpty => "BucketNotEmpty",
            Self::InternalError => "InternalError",
            Self::InvalidAccessKeyId => "InvalidAccessKeyId",
            Self::InvalidArgument => "InvalidArgument",
            Self::InvalidBucketName => "InvalidBucketName",
            Self::InvalidPart => "InvalidPart",
            Self::MalformedXML => "MalformedXML",
            Self::NoSuchBucket => "NoSuchBucket",
            Self::NoSuchKey => "NoSuchKey",
            Self::NoSuchUpload => "NoSuchUpload",
            Self::NoSuchVersion => "NoSuchVersion",
            Self::InvalidRange => "InvalidRange",
            Self::NotImplemented => "NotImplemented",
            Self::EntityTooSmall => "EntityTooSmall",
            Self::EntityTooLarge => "EntityTooLarge",
            Self::InsufficientStorage => "InsufficientStorage",
            Self::ExpiredPresignedUrl => "AccessDenied",
            Self::PreconditionFailed => "PreconditionFailed",
            Self::NoSuchCORSConfiguration => "NoSuchCORSConfiguration",
            Self::NoSuchBucketPolicy => "NoSuchBucketPolicy",
            Self::MalformedPolicy => "MalformedPolicy",
            Self::SignatureDoesNotMatch => "SignatureDoesNotMatch",
            Self::InvalidEncryptionAlgorithm => "InvalidEncryptionAlgorithmError",
            Self::ServerSideEncryptionConfigurationNotFound => {
                "ServerSideEncryptionConfigurationNotFoundError"
            }
            Self::SlowDown => "SlowDown",
        }
    }

    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::AccessDenied
            | Self::ExpiredPresignedUrl
            | Self::InvalidAccessKeyId
            | Self::SignatureDoesNotMatch => StatusCode::FORBIDDEN,
            Self::NoSuchBucket
            | Self::NoSuchKey
            | Self::NoSuchUpload
            | Self::NoSuchVersion
            | Self::NoSuchCORSConfiguration
            | Self::NoSuchBucketPolicy
            | Self::ServerSideEncryptionConfigurationNotFound => StatusCode::NOT_FOUND,
            Self::MalformedPolicy => StatusCode::BAD_REQUEST,
            Self::BucketAlreadyOwnedByYou | Self::BucketNotEmpty => StatusCode::CONFLICT,
            Self::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InsufficientStorage => StatusCode::INSUFFICIENT_STORAGE,
            Self::SlowDown => StatusCode::TOO_MANY_REQUESTS,
            Self::InvalidRange => StatusCode::RANGE_NOT_SATISFIABLE,
            Self::NotImplemented => StatusCode::NOT_IMPLEMENTED,
            Self::PreconditionFailed => StatusCode::PRECONDITION_FAILED,
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

impl S3Error {
    pub fn internal(err: impl std::fmt::Display) -> Self {
        tracing::error!("Internal error: {}", err);
        Self {
            code: S3ErrorCode::InternalError,
            message: "We encountered an internal error. Please try again.".into(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn no_such_bucket(bucket: &str) -> Self {
        Self {
            code: S3ErrorCode::NoSuchBucket,
            message: format!("The specified bucket does not exist: {}", bucket),
            resource: Some(format!("/{}", bucket)),
            retry_after_secs: None,
        }
    }

    pub fn no_such_key(key: &str) -> Self {
        Self {
            code: S3ErrorCode::NoSuchKey,
            message: "The specified key does not exist.".into(),
            resource: Some(key.to_string()),
            retry_after_secs: None,
        }
    }

    pub fn no_such_upload(upload_id: &str) -> Self {
        Self {
            code: S3ErrorCode::NoSuchUpload,
            message: "The specified multipart upload does not exist.".into(),
            resource: Some(upload_id.to_string()),
            retry_after_secs: None,
        }
    }

    pub fn bucket_already_owned(bucket: &str) -> Self {
        Self {
            code: S3ErrorCode::BucketAlreadyOwnedByYou,
            message: format!(
                "Your previous request to create the named bucket succeeded and you already own it: {}",
                bucket
            ),
            resource: Some(format!("/{}", bucket)),
            retry_after_secs: None,
        }
    }

    pub fn bucket_not_empty(bucket: &str) -> Self {
        Self {
            code: S3ErrorCode::BucketNotEmpty,
            message: "The bucket you tried to delete is not empty.".into(),
            resource: Some(format!("/{}", bucket)),
            retry_after_secs: None,
        }
    }

    pub fn invalid_bucket_name(name: &str) -> Self {
        Self {
            code: S3ErrorCode::InvalidBucketName,
            message: format!("The specified bucket is not valid: {}", name),
            resource: Some(format!("/{}", name)),
            retry_after_secs: None,
        }
    }

    pub fn invalid_argument(msg: &str) -> Self {
        Self {
            code: S3ErrorCode::InvalidArgument,
            message: msg.to_string(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn bad_digest() -> Self {
        Self {
            code: S3ErrorCode::BadDigest,
            message: "The Content-MD5 you specified did not match what we received.".into(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn bad_checksum(algo: &str) -> Self {
        Self {
            code: S3ErrorCode::BadDigest,
            message: format!(
                "The {} checksum you specified did not match what we received.",
                algo
            ),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn malformed_xml() -> Self {
        Self {
            code: S3ErrorCode::MalformedXML,
            message: "The XML you provided was not well-formed.".into(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn invalid_part(msg: &str) -> Self {
        Self {
            code: S3ErrorCode::InvalidPart,
            message: msg.to_string(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn entity_too_small() -> Self {
        Self {
            code: S3ErrorCode::EntityTooSmall,
            message: "Your proposed upload is smaller than the minimum allowed object size.".into(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn entity_too_large(max: u64) -> Self {
        Self {
            code: S3ErrorCode::EntityTooLarge,
            message: format!(
                "Your proposed upload exceeds the maximum allowed object size of {} bytes.",
                max
            ),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn insufficient_storage(msg: &str) -> Self {
        Self {
            code: S3ErrorCode::InsufficientStorage,
            message: msg.to_string(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn expired_presigned_url() -> Self {
        Self {
            code: S3ErrorCode::ExpiredPresignedUrl,
            message: "Request has expired".into(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn access_denied(msg: &str) -> Self {
        Self {
            code: S3ErrorCode::AccessDenied,
            message: msg.to_string(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn signature_mismatch() -> Self {
        Self {
            code: S3ErrorCode::SignatureDoesNotMatch,
            message:
                "The request signature we calculated does not match the signature you provided."
                    .into(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn invalid_access_key() -> Self {
        Self {
            code: S3ErrorCode::InvalidAccessKeyId,
            message: "The AWS Access Key Id you provided does not exist in our records.".into(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn no_such_version(version_id: &str) -> Self {
        Self {
            code: S3ErrorCode::NoSuchVersion,
            message: "The specified version does not exist.".into(),
            resource: Some(version_id.to_string()),
            retry_after_secs: None,
        }
    }

    pub fn invalid_range() -> Self {
        Self {
            code: S3ErrorCode::InvalidRange,
            message: "The requested range is not satisfiable".into(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn not_implemented(msg: &str) -> Self {
        Self {
            code: S3ErrorCode::NotImplemented,
            message: msg.to_string(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn no_such_cors_configuration() -> Self {
        Self {
            code: S3ErrorCode::NoSuchCORSConfiguration,
            message: "The CORS configuration does not exist".into(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn no_such_bucket_policy() -> Self {
        Self {
            code: S3ErrorCode::NoSuchBucketPolicy,
            message: "The bucket policy does not exist".into(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn malformed_policy(msg: impl Into<String>) -> Self {
        Self {
            code: S3ErrorCode::MalformedPolicy,
            message: msg.into(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn precondition_failed() -> Self {
        Self {
            code: S3ErrorCode::PreconditionFailed,
            message: "At least one of the pre-conditions you specified did not hold.".into(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn invalid_encryption_algorithm() -> Self {
        Self {
            code: S3ErrorCode::InvalidEncryptionAlgorithm,
            message: "The encryption request you specified is not valid. Supported value: AES256."
                .into(),
            resource: None,
            retry_after_secs: None,
        }
    }

    pub fn no_such_bucket_encryption(bucket: &str) -> Self {
        Self {
            code: S3ErrorCode::ServerSideEncryptionConfigurationNotFound,
            message: "The server side encryption configuration was not found.".into(),
            resource: Some(format!("/{}", bucket)),
            retry_after_secs: None,
        }
    }

    pub fn slow_down(retry_after_secs: u64) -> Self {
        Self {
            code: S3ErrorCode::SlowDown,
            message: "Please reduce your request rate.".into(),
            resource: None,
            retry_after_secs: Some(retry_after_secs),
        }
    }
}

impl IntoResponse for S3Error {
    fn into_response(self) -> Response {
        let resource = self.resource.as_deref().unwrap_or("");
        let request_id = uuid::Uuid::new_v4();
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <Error>\
             <Code>{}</Code>\
             <Message>{}</Message>\
             <Resource>{}</Resource>\
             <RequestId>{}</RequestId>\
             </Error>",
            self.code.as_str(),
            quick_xml::escape::escape(&self.message),
            quick_xml::escape::escape(resource),
            request_id,
        );

        let status = self.code.status_code();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/xml"),
        );
        if let Ok(value) = HeaderValue::from_str(&request_id.to_string()) {
            headers.insert("x-amz-request-id", value);
        }
        if let Some(retry_after) = self.retry_after_secs {
            if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
                headers.insert(header::RETRY_AFTER, value);
            }
        }
        (status, headers, xml).into_response()
    }
}
