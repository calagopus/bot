use aws_sdk_s3::{
    Client,
    config::{BehaviorVersion, Config, Credentials, Region, retry::RetryConfig},
    primitives::ByteStream,
};

#[derive(Debug)]
pub struct Storage {
    client: Client,
    bucket: String,
    public_url: String,
}

impl Storage {
    pub fn new(env: &crate::env::Env) -> Option<Self> {
        let (endpoint, bucket, access_key, secret_key, public_url) = (
            env.s3_endpoint.as_ref()?,
            env.s3_bucket.as_ref()?,
            env.s3_access_key.as_ref()?,
            env.s3_secret_key.as_ref()?,
            env.s3_public_url.as_ref()?,
        );

        let config = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .credentials_provider(Credentials::new(
                access_key,
                secret_key,
                None,
                None,
                "calagopus-bot",
            ))
            .region(Region::new(env.s3_region.clone()))
            .endpoint_url(endpoint)
            .force_path_style(env.s3_path_style)
            .retry_config(RetryConfig::standard())
            .build();

        Some(Self {
            client: Client::from_conf(config),
            bucket: bucket.clone(),
            public_url: public_url.trim_end_matches('/').to_string(),
        })
    }

    pub async fn store(
        &self,
        path: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<(), anyhow::Error> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(path)
            .content_type(content_type)
            .body(ByteStream::from(bytes))
            .send()
            .await?;

        Ok(())
    }

    pub async fn remove(&self, path: &str) -> Result<(), anyhow::Error> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await?;

        Ok(())
    }

    #[inline]
    pub fn url(&self, path: &str) -> String {
        format!("{}/{}", self.public_url, path)
    }
}
