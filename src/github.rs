use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub struct Client {
    http: reqwest::Client,
    base_url: String,
}

// --- GraphQL response types ---

#[derive(Debug, Deserialize)]
struct GraphQLResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQLError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct RepositoryData {
    repository: RepositoryNode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryNode {
    pull_request: GraphQLPullRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQLPullRequest {
    number: u64,
    title: String,
    body: Option<String>,
    state: String,
    additions: u64,
    deletions: u64,
    changed_files: u64,
    head_ref_name: String,
    base_ref_name: String,
    head_ref_oid: String,
    files: FileConnection,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileConnection {
    page_info: PageInfo,
    nodes: Vec<GraphQLPrFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQLPrFile {
    path: String,
    additions: u64,
    deletions: u64,
    change_type: String,
}

// --- Pagination query for additional file pages ---

#[derive(Debug, Deserialize)]
struct FilesPageData {
    repository: FilesPageRepository,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilesPageRepository {
    pull_request: FilesPagePR,
}

#[derive(Debug, Deserialize)]
struct FilesPagePR {
    files: FileConnection,
}

// --- REST file type (has patch) ---

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RestPrFile {
    filename: String,
    status: String,
    additions: u64,
    deletions: u64,
    patch: Option<String>,
}

// --- Public types ---

#[derive(Debug, Clone)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
    pub head_ref: String,
    pub base_ref: String,
    pub head_sha: String,
    pub files: Vec<PrFile>,
}

#[derive(Debug, Clone)]
pub struct PrFile {
    pub filename: String,
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    pub patch: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FileContent {
    pub content: Option<String>,
    #[allow(dead_code)]
    pub encoding: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateReview {
    pub commit_id: String,
    pub event: String,
    pub body: String,
    pub comments: Vec<ReviewCommentInput>,
}

#[derive(Debug, Serialize)]
pub struct ReviewCommentInput {
    pub path: String,
    pub line: u64,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateReviewResponse {
    pub id: u64,
    pub html_url: String,
}

#[derive(Debug, Deserialize)]
pub struct CodeSearchResponse {
    pub total_count: u64,
    pub items: Vec<CodeSearchItem>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CodeSearchItem {
    pub name: String,
    pub path: String,
    pub repository: CodeSearchRepo,
    pub html_url: String,
    pub text_matches: Option<Vec<TextMatch>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CodeSearchRepo {
    pub full_name: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct TextMatch {
    pub fragment: String,
    pub matches: Vec<TextMatchLocation>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct TextMatchLocation {
    pub indices: Vec<u64>,
}

/// Parse a raw unified diff string into a map of filename -> patch content
fn parse_raw_diff(raw: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut current_file: Option<String> = None;
    let mut current_patch = String::new();

    for line in raw.lines() {
        if line.starts_with("diff --git ") {
            // Save previous file's patch
            if let Some(file) = current_file.take() {
                if !current_patch.is_empty() {
                    map.insert(file, current_patch.trim_start_matches('\n').to_string());
                }
            }
            current_patch = String::new();
        } else if line.starts_with("+++ b/") {
            current_file = Some(line[6..].to_string());
        } else if line.starts_with("@@") || current_file.is_some() && !line.starts_with("--- ") && !line.starts_with("+++ ") && !line.starts_with("index ") && !line.starts_with("new file") && !line.starts_with("deleted file") && !line.starts_with("old mode") && !line.starts_with("new mode") && !line.starts_with("similarity") && !line.starts_with("rename ") {
            if current_file.is_some() {
                if !current_patch.is_empty() {
                    current_patch.push('\n');
                }
                current_patch.push_str(line);
            }
        }
    }

    // Save last file
    if let Some(file) = current_file {
        if !current_patch.is_empty() {
            map.insert(file, current_patch.trim_start_matches('\n').to_string());
        }
    }

    map
}

fn map_change_type(ct: &str) -> String {
    match ct {
        "ADDED" => "added".to_string(),
        "DELETED" | "REMOVED" => "removed".to_string(),
        "MODIFIED" | "CHANGED" => "modified".to_string(),
        "RENAMED" => "renamed".to_string(),
        "COPIED" => "copied".to_string(),
        other => other.to_lowercase(),
    }
}

fn split_repo(repo: &str) -> Result<(&str, &str)> {
    repo.split_once('/')
        .ok_or_else(|| anyhow::anyhow!("Repository must be in owner/repo format, got: {repo}"))
}

impl Client {
    pub fn new() -> Result<Self> {
        let token = std::env::var("GITHUB_TOKEN")
            .or_else(|_| Self::token_from_gh_cli())
            .context("Set GITHUB_TOKEN or install/auth gh CLI")?;

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))?,
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("gh-agent/0.1"));
        headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static("2022-11-28"),
        );

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self {
            http,
            base_url: "https://api.github.com".to_string(),
        })
    }

    fn token_from_gh_cli() -> Result<String> {
        let output = std::process::Command::new("gh")
            .args(["auth", "token"])
            .output()
            .context("Failed to run `gh auth token`")?;
        if !output.status.success() {
            anyhow::bail!("gh auth token failed");
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    // --- GraphQL ---

    async fn graphql<T: DeserializeOwned>(&self, query: &str, variables: &serde_json::Value) -> Result<T> {
        let body = serde_json::json!({
            "query": query,
            "variables": variables,
        });
        let url = format!("{}/graphql", self.base_url);
        let resp = self.http.post(&url).json(&body).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub GraphQL error {status}: {text}");
        }
        let gql_resp: GraphQLResponse<T> = resp.json().await?;
        if let Some(errors) = gql_resp.errors {
            let msgs: Vec<String> = errors.into_iter().map(|e| e.message).collect();
            anyhow::bail!("GraphQL errors: {}", msgs.join("; "));
        }
        gql_resp.data.ok_or_else(|| anyhow::anyhow!("No data in GraphQL response"))
    }

    // --- REST helpers ---

    async fn rest_get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub API error {status}: {body}");
        }
        Ok(resp.json().await?)
    }

    async fn rest_get_all_pages<T: DeserializeOwned>(&self, path: &str) -> Result<Vec<T>> {
        let mut all = Vec::new();
        let mut page = 1u32;
        loop {
            let sep = if path.contains('?') { '&' } else { '?' };
            let url = format!(
                "{}{}{}per_page=100&page={}",
                self.base_url, path, sep, page
            );
            let resp = self.http.get(&url).send().await?;
            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("GitHub API error {status}: {body}");
            }
            let items: Vec<T> = resp.json().await?;
            if items.is_empty() {
                break;
            }
            all.extend(items);
            page += 1;
        }
        Ok(all)
    }

    async fn rest_post<B: Serialize, R: DeserializeOwned>(&self, path: &str, body: &B) -> Result<R> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http.post(&url).json(body).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub API error {status}: {body}");
        }
        Ok(resp.json().await?)
    }

    // --- Public API ---

    /// Fetch PR metadata + file list via GraphQL (no patches — fast)
    pub async fn get_pr(&self, repo: &str, number: u64) -> Result<PullRequest> {
        let (owner, name) = split_repo(repo)?;

        const QUERY: &str = r#"
query($owner: String!, $repo: String!, $number: Int!) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      number
      title
      body
      state
      additions
      deletions
      changedFiles
      headRefName
      baseRefName
      headRefOid
      files(first: 100) {
        pageInfo { hasNextPage endCursor }
        nodes {
          path
          additions
          deletions
          changeType
        }
      }
    }
  }
}
"#;

        let vars = serde_json::json!({
            "owner": owner,
            "repo": name,
            "number": number as i64,
        });

        let data: RepositoryData = self.graphql(QUERY, &vars).await?;
        let pr = data.repository.pull_request;

        let mut files: Vec<PrFile> = pr.files.nodes.iter().map(|f| PrFile {
            filename: f.path.clone(),
            status: map_change_type(&f.change_type),
            additions: f.additions,
            deletions: f.deletions,
            patch: None,
        }).collect();

        // Paginate remaining files
        let mut page_info = pr.files.page_info;
        while page_info.has_next_page {
            let cursor = page_info.end_cursor.as_deref().unwrap_or_default();
            let more = self.get_pr_files_page(owner, name, number, cursor).await?;
            for f in &more.nodes {
                files.push(PrFile {
                    filename: f.path.clone(),
                    status: map_change_type(&f.change_type),
                    additions: f.additions,
                    deletions: f.deletions,
                    patch: None,
                });
            }
            page_info = more.page_info;
        }

        Ok(PullRequest {
            number: pr.number,
            title: pr.title,
            body: pr.body,
            state: pr.state,
            additions: pr.additions,
            deletions: pr.deletions,
            changed_files: pr.changed_files,
            head_ref: pr.head_ref_name,
            base_ref: pr.base_ref_name,
            head_sha: pr.head_ref_oid,
            files,
        })
    }

    async fn get_pr_files_page(
        &self,
        owner: &str,
        name: &str,
        number: u64,
        cursor: &str,
    ) -> Result<FileConnection> {
        const QUERY: &str = r#"
query($owner: String!, $repo: String!, $number: Int!, $cursor: String!) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      files(first: 100, after: $cursor) {
        pageInfo { hasNextPage endCursor }
        nodes {
          path
          additions
          deletions
          changeType
        }
      }
    }
  }
}
"#;
        let vars = serde_json::json!({
            "owner": owner,
            "repo": name,
            "number": number as i64,
            "cursor": cursor,
        });

        let data: FilesPageData = self.graphql(QUERY, &vars).await?;
        Ok(data.repository.pull_request.files)
    }

    /// Fetch the raw unified diff for a PR (single request, no pagination)
    async fn get_pr_raw_diff(&self, repo: &str, number: u64) -> Result<String> {
        let url = format!("{}/repos/{}/pulls/{}", self.base_url, repo, number);
        let resp = self.http
            .get(&url)
            .header(ACCEPT, "application/vnd.github.diff")
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub API error {status}: {body}");
        }
        Ok(resp.text().await?)
    }

    /// Fetch PR metadata (GraphQL) + raw diff (REST) in parallel
    pub async fn get_pr_with_patches(&self, repo: &str, number: u64) -> Result<PullRequest> {
        let (pr, raw_diff) = tokio::try_join!(
            self.get_pr(repo, number),
            self.get_pr_raw_diff(repo, number),
        )?;

        // Parse raw unified diff into per-file patches
        let patch_map = parse_raw_diff(&raw_diff);

        let files = pr.files.into_iter().map(|mut f| {
            if let Some(patch) = patch_map.get(&f.filename) {
                f.patch = Some(patch.clone());
            }
            f
        }).collect();

        Ok(PullRequest {
            files,
            ..pr
        })
    }

    pub async fn get_file_content(
        &self,
        repo: &str,
        path: &str,
        git_ref: &str,
    ) -> Result<String> {
        let fc: FileContent = self
            .rest_get(&format!("/repos/{repo}/contents/{path}?ref={git_ref}"))
            .await?;
        let encoded = fc.content.unwrap_or_default();
        let cleaned: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &cleaned)?;
        Ok(String::from_utf8(bytes)?)
    }

    /// Fetch before/after contents for a list of files.
    /// Returns Vec of (filename, status, before_content, after_content).
    /// Fetches all files concurrently. Silently skips files that fail (binary, too large, etc).
    pub async fn get_file_pairs(
        &self,
        repo: &str,
        files: &[PrFile],
        base_ref: &str,
        head_ref: &str,
    ) -> Vec<(String, String, Option<String>, Option<String>)> {
        let futs: Vec<_> = files
            .iter()
            .map(|f| {
                let filename = f.filename.clone();
                let status = f.status.clone();
                let repo = repo.to_string();
                let base = base_ref.to_string();
                let head = head_ref.to_string();

                async move {
                    let before = if status == "added" {
                        None
                    } else {
                        self.get_file_content(&repo, &filename, &base).await.ok()
                    };

                    let after = if status == "removed" {
                        None
                    } else {
                        self.get_file_content(&repo, &filename, &head).await.ok()
                    };

                    (filename, status, before, after)
                }
            })
            .collect();

        futures::future::join_all(futs).await
    }

    /// Search code in a repo via GitHub Code Search API (searches default branch).
    /// Returns up to 100 results (API limit per page).
    pub async fn search_code(&self, repo: &str, query: &str, path_prefix: Option<&str>) -> Result<CodeSearchResponse> {
        let mut q = format!("{} repo:{}", query, repo);
        if let Some(prefix) = path_prefix {
            q.push_str(&format!(" path:{}", prefix));
        }

        let encoded_q = urlencoding::encode(&q);
        let url = format!("{}/search/code?q={}&per_page=100", self.base_url, encoded_q);

        let resp = self.http
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/vnd.github.text-match+json")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub Code Search error {status}: {body}");
        }

        Ok(resp.json().await?)
    }

    pub async fn create_review(
        &self,
        repo: &str,
        number: u64,
        review: &CreateReview,
    ) -> Result<CreateReviewResponse> {
        self.rest_post(&format!("/repos/{repo}/pulls/{number}/reviews"), review)
            .await
    }
}
