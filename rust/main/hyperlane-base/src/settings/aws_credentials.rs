#![allow(clippy::doc_markdown)] // TODO: `rustc` 1.80.1 clippy issue
#![allow(clippy::doc_lazy_continuation)] // TODO: `rustc` 1.80.1 clippy issue

use async_trait::async_trait;
use rusoto_core::credential::{AwsCredentials, CredentialsError, ProvideAwsCredentials};

/// Provides AWS credentials from multiple possible sources using a priority order.
/// The following sources are checked in order for credentials when calling credentials. More sources may be supported in future if a need be.
/// 1) Environment variables: `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`.
/// 2) `WebIdentityProvider`: by default, configured from environment variables `AWS_WEB_IDENTITY_TOKEN_FILE`,
/// `AWS_ROLE_ARN` and `AWS_ROLE_SESSION_NAME`. Uses OpenID Connect bearer token to retrieve AWS IAM credentials
/// from [AssumeRoleWithWebIdentity](https://docs.aws.amazon.com/STS/latest/APIReference/API_AssumeRoleWithWebIdentity.html).
/// The primary use case is running Hyperlane agents in AWS Kubernetes cluster (EKS) configured
/// with [IAM Roles for Service Accounts (IRSA)](https://aws.amazon.com/blogs/containers/diving-into-iam-roles-for-service-accounts/).
/// The IRSA approach follows security best practices and allows for key rotation.
pub(crate) struct AwsChainCredentialsProvider {
    pub key: String,
    pub secret: String,
}

impl AwsChainCredentialsProvider {
    pub fn new(key: &str, secret: &str) -> Self {
        return AwsChainCredentialsProvider {
            key: key.to_string(),
            secret: secret.to_string(),
        };
    }
}

#[async_trait]
impl ProvideAwsCredentials for AwsChainCredentialsProvider {
    async fn credentials(&self) -> Result<AwsCredentials, CredentialsError> {
        let credentials = AwsCredentials::new(self.key.clone(), self.secret.clone(), None, None);
        return Ok(credentials);
    }
}
