//! A última release publicada no GitHub (pré-releases e rascunhos ficam de fora).

use elgin_application::ports::{Release, ReleaseAsset, UpdateError, UpdateSource};
use serde::Deserialize;

use crate::http::HttpGet;

/// `GET /repos/{dono}/{repo}/releases/latest`.
pub const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/BrunoNC22/elgin-tray/releases/latest";

/// Fonte de versões: as releases do repositório no GitHub.
#[derive(Debug, Clone)]
pub struct GitHubReleases<H> {
    http: H,
    url: String,
}

impl<H: HttpGet> GitHubReleases<H> {
    pub fn new(http: H) -> Self {
        Self::with_url(http, LATEST_RELEASE_URL.to_owned())
    }

    /// Com outro endereço no lugar da API do GitHub (testes manuais com um servidor local).
    pub fn with_url(http: H, url: String) -> Self {
        Self { http, url }
    }
}

#[derive(Debug, Deserialize)]
struct ReleaseJson {
    tag_name: String,
    assets: Vec<AssetJson>,
}

#[derive(Debug, Deserialize)]
struct AssetJson {
    name: String,
    browser_download_url: String,
}

impl<H: HttpGet> UpdateSource for GitHubReleases<H> {
    async fn latest_release(&mut self) -> Result<Release, UpdateError> {
        let response = self
            .http
            .get(&self.url)
            .await
            .map_err(UpdateError::Network)?;
        match response.status {
            200 => {}
            404 => {
                return Err(UpdateError::Rejected(
                    "nenhuma versão publicada ainda".to_owned(),
                ));
            }
            // 403/429: limite de requisições do GitHub; 5xx: fora do ar. Tenta na próxima hora.
            status => return Err(UpdateError::Network(format!("GitHub respondeu {status}"))),
        }
        let json: ReleaseJson = serde_json::from_slice(&response.body).map_err(|error| {
            UpdateError::Rejected(format!("resposta inesperada do GitHub ({error})"))
        })?;
        let version = json
            .tag_name
            .parse()
            .map_err(|error| UpdateError::Rejected(format!("tag da release: {error}")))?;
        Ok(Release {
            version,
            assets: json
                .assets
                .into_iter()
                .map(|asset| ReleaseAsset {
                    name: asset.name,
                    url: asset.browser_download_url,
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::testing::FakeHttp;

    fn source_with(status: u16, body: &str) -> GitHubReleases<FakeHttp> {
        let mut http = FakeHttp::default();
        http.respond(LATEST_RELEASE_URL, status, body);
        GitHubReleases::new(http)
    }

    #[tokio::test]
    async fn reads_the_version_and_the_assets() {
        let mut source = source_with(
            200,
            r#"{
                "tag_name": "v1.2.3",
                "name": "elgin-tray v1.2.3",
                "assets": [{
                    "name": "elgin-tray-v1.2.3-windows-x86_64.zip",
                    "size": 123,
                    "browser_download_url": "https://github.com/x/releases/download/v1.2.3/w.zip"
                }]
            }"#,
        );

        let release = source.latest_release().await.unwrap();

        assert_eq!(release.version.to_string(), "1.2.3");
        assert_eq!(
            release.assets,
            [ReleaseAsset {
                name: "elgin-tray-v1.2.3-windows-x86_64.zip".to_owned(),
                url: "https://github.com/x/releases/download/v1.2.3/w.zip".to_owned(),
            }]
        );
    }

    #[tokio::test]
    async fn without_releases_there_is_nothing_to_install() {
        let mut source = source_with(404, r#"{"message": "Not Found"}"#);

        assert!(matches!(
            source.latest_release().await,
            Err(UpdateError::Rejected(reason)) if reason.contains("nenhuma versão")
        ));
    }

    #[tokio::test]
    async fn rate_limits_and_network_errors_are_temporary() {
        let mut limited = source_with(403, r#"{"message": "API rate limit exceeded"}"#);
        assert!(matches!(
            limited.latest_release().await,
            Err(UpdateError::Network(_))
        ));

        let mut offline = GitHubReleases::new(FakeHttp::default());
        assert!(matches!(
            offline.latest_release().await,
            Err(UpdateError::Network(_))
        ));
    }

    #[tokio::test]
    async fn an_unexpected_answer_is_rejected() {
        for body in ["<html>", r#"{"tag_name": "nightly", "assets": []}"#] {
            let mut source = source_with(200, body);
            assert!(matches!(
                source.latest_release().await,
                Err(UpdateError::Rejected(_))
            ));
        }
    }
}
