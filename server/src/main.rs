use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Local, NaiveDateTime, TimeZone};
use serde::Deserialize;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

// ========================= 42 Header Template =========================

/// The generic 42 header template with `*` delimiters.
/// Each line is exactly 80 characters. Fields are `$NAME___` placeholders.
const GENERIC_TEMPLATE: &str = "\
********************************************************************************\n\
*                                                                              *\n\
*                                                         :::      ::::::::    *\n\
*    $FILENAME__________________________________        :+:      :+:    :+:    *\n\
*                                                     +:+ +:+         +:+      *\n\
*    By: $AUTHOR________________________________    +#+  +:+       +#+         *\n\
*                                                 +#+#+#+#+#+   +#+            *\n\
*    Created: $CREATEDAT_________ by $CREATEDBY_       #+#    #+#              *\n\
*    Updated: $UPDATEDAT_________ by $UPDATEDBY_      ###   ########.fr        *\n\
*                                                                              *\n\
********************************************************************************\n\
\n";

// ========================= Configuration =========================

/// Configuration loaded from multiple sources with priority:
/// 1. Zed settings (initialization_options) — highest
/// 2. Config file (~/.config/42header/config.toml)
/// 3. Environment variables (USER42/USER, MAIL42)
/// 4. Defaults — lowest
#[derive(Deserialize, Default, Clone)]
struct UserConfig {
    username: Option<String>,
    email: Option<String>,
}

/// Paths where we look for a config file, in order
fn config_file_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // XDG config
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        paths.push(PathBuf::from(xdg).join("42header/config.toml"));
    }

    // ~/.config/42header/config.toml
    if let Some(home) = home_dir() {
        paths.push(home.join(".config/42header/config.toml"));
    }

    // ~/.42header.toml
    if let Some(home) = home_dir() {
        paths.push(home.join(".42header.toml"));
    }

    paths
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

fn load_config_file() -> UserConfig {
    for path in config_file_paths() {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Ok(config) = toml::from_str::<UserConfig>(&contents) {
                return config;
            }
        }
    }
    UserConfig::default()
}

fn load_env_config() -> UserConfig {
    UserConfig {
        username: std::env::var("USER42")
            .ok()
            .or_else(|| std::env::var("USER").ok())
            .or_else(|| std::env::var("USERNAME").ok()),
        email: std::env::var("MAIL42").ok(),
    }
}

/// Merge config sources: first non-None wins
fn resolve_config(init_opts: UserConfig, file_cfg: UserConfig, env_cfg: UserConfig) -> (String, String) {
    let username = init_opts
        .username
        .or(file_cfg.username)
        .or(env_cfg.username)
        .unwrap_or_else(|| "marvin".to_string());

    let email = init_opts
        .email
        .or(file_cfg.email)
        .or(env_cfg.email)
        .unwrap_or_else(|| format!("{}@student.42.fr", username));

    (username, email)
}

/// Returns the path where a new config file should be created
fn default_config_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("42header/config.toml")
    } else if let Some(home) = home_dir() {
        home.join(".config/42header/config.toml")
    } else {
        PathBuf::from(".42header.toml")
    }
}

// ========================= Delimiter Mappings =========================

fn get_delimiters(language_id: &str, filename: &str) -> (&'static str, &'static str) {
    let slashes = ("/* ", " */");
    let hashes = ("# ", " #");
    let dashes = ("-- ", " --");
    let parens = ("(* ", " *)");
    let percents = ("%% ", " %%");
    let semicolons = (";; ", " ;;");

    match language_id.to_lowercase().as_str() {
        "c" | "cpp" | "c++" | "css" | "go" | "groovy" | "java" | "javascript"
        | "javascriptreact" | "jsx" | "less" | "objective-c" | "objective_c" | "objc"
        | "php" | "rust" | "scss" | "swift" | "typescript" | "typescriptreact" | "tsx"
        | "xsl" | "jade" => slashes,

        "python" | "ruby" | "perl" | "perl6" | "bash" | "shellscript" | "shell"
        | "shell script" | "sh" | "zsh" | "fish" | "makefile" | "make" | "coffeescript"
        | "powershell" | "r" | "sql" | "yaml" | "dockerfile" | "plaintext"
        | "plain text" | "toml" | "gitignore" | "env" => hashes,

        "haskell" | "lua" => dashes,
        "ocaml" | "fsharp" | "f#" => parens,
        "latex" | "tex" => percents,
        "ini" => semicolons,

        _ => match filename.rsplit('.').next().unwrap_or("") {
            "c" | "h" => slashes,
            "cpp" | "hpp" | "cc" | "cxx" | "hxx" | "hh" => slashes,
            "css" | "scss" | "less" => slashes,
            "go" => slashes,
            "java" => slashes,
            "js" | "jsx" | "mjs" | "cjs" => slashes,
            "ts" | "tsx" | "mts" | "cts" => slashes,
            "php" => slashes,
            "rs" => slashes,
            "swift" => slashes,
            "m" | "mm" => slashes,
            "groovy" | "gradle" => slashes,
            "xsl" | "xslt" => slashes,
            "py" | "pyw" => hashes,
            "rb" => hashes,
            "pl" | "pm" => hashes,
            "sh" | "bash" | "zsh" | "fish" => hashes,
            "mk" | "makefile" => hashes,
            "coffee" => hashes,
            "ps1" => hashes,
            "r" => hashes,
            "sql" => hashes,
            "yml" | "yaml" => hashes,
            "toml" => hashes,
            "hs" => dashes,
            "lua" => dashes,
            "ml" | "mli" => parens,
            "fs" | "fsi" | "fsx" => parens,
            "tex" | "sty" | "cls" => percents,
            "ini" | "cfg" => semicolons,
            _ => slashes,
        },
    }
}

// ========================= Header Logic =========================

struct HeaderInfo {
    filename: String,
    author: String,
    created_by: String,
    created_at: chrono::DateTime<Local>,
    updated_by: String,
    updated_at: chrono::DateTime<Local>,
}

fn find_field(name: &str) -> (usize, usize) {
    let marker = format!("${}", name);
    let offset = GENERIC_TEMPLATE
        .find(&marker)
        .unwrap_or_else(|| panic!("field ${} not found in template", name));
    let after = offset + marker.len();
    let underscores = GENERIC_TEMPLATE[after..]
        .chars()
        .take_while(|&c| c == '_')
        .count();
    (offset, marker.len() + underscores)
}

fn pad(value: &str, width: usize) -> String {
    let mut s = String::new();
    for c in value.chars() {
        if s.len() + c.len_utf8() > width {
            break;
        }
        s.push(c);
    }
    while s.len() < width {
        s.push(' ');
    }
    s
}

fn get_field_value(header: &str, name: &str) -> String {
    let (offset, width) = find_field(name);
    if offset + width <= header.len() {
        header[offset..offset + width].trim_end().to_string()
    } else {
        String::new()
    }
}

fn set_field_value(header: &str, name: &str, value: &str) -> String {
    let (offset, width) = find_field(name);
    let padded = pad(value, width);
    let mut result = String::with_capacity(header.len());
    result.push_str(&header[..offset]);
    result.push_str(&padded);
    result.push_str(&header[offset + width..]);
    result
}

fn apply_delimiters(template: &str, left: &str, right: &str) -> String {
    let width = left.len();
    let mut result = String::with_capacity(template.len());
    for line in template.lines() {
        if line.len() >= 2 * width {
            result.push_str(left);
            result.push_str(&line[width..line.len() - width]);
            result.push_str(right);
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }
    result
}

fn format_date(dt: &chrono::DateTime<Local>) -> String {
    dt.format("%Y/%m/%d %H:%M:%S").to_string()
}

fn parse_date(s: &str) -> chrono::DateTime<Local> {
    NaiveDateTime::parse_from_str(s.trim(), "%Y/%m/%d %H:%M:%S")
        .ok()
        .and_then(|ndt| Local.from_local_datetime(&ndt).single())
        .unwrap_or_else(Local::now)
}

fn extract_header(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().take(11).collect();
    if lines.len() < 10 {
        return None;
    }
    for i in 0..10 {
        if lines[i].len() != 80 {
            return None;
        }
    }
    let mut result = String::new();
    for i in 0..10 {
        result.push_str(lines[i]);
        result.push('\n');
    }
    Some(result)
}

/// Fixed end line for header replacement (matches VS Code Range(0,0,12,0))
const HEADER_END_LINE: u32 = 12;

fn get_header_info(header: &str) -> HeaderInfo {
    HeaderInfo {
        filename: get_field_value(header, "FILENAME"),
        author: get_field_value(header, "AUTHOR"),
        created_by: get_field_value(header, "CREATEDBY"),
        created_at: parse_date(&get_field_value(header, "CREATEDAT")),
        updated_by: get_field_value(header, "UPDATEDBY"),
        updated_at: parse_date(&get_field_value(header, "UPDATEDAT")),
    }
}

fn render_header(language_id: &str, filename: &str, info: &HeaderInfo) -> String {
    let (left, right) = get_delimiters(language_id, filename);
    let template = apply_delimiters(GENERIC_TEMPLATE, left, right);
    let header = set_field_value(&template, "FILENAME", &info.filename);
    let header = set_field_value(&header, "AUTHOR", &info.author);
    let header = set_field_value(&header, "CREATEDAT", &format_date(&info.created_at));
    let header = set_field_value(&header, "CREATEDBY", &info.created_by);
    let header = set_field_value(&header, "UPDATEDAT", &format_date(&info.updated_at));
    set_field_value(&header, "UPDATEDBY", &info.updated_by)
}

// ========================= LSP Server =========================

struct DocumentState {
    content: String,
    language_id: String,
}

struct Backend {
    client: Client,
    documents: Arc<RwLock<HashMap<Url, DocumentState>>>,
    username: Arc<RwLock<String>>,
    email: Arc<RwLock<String>>,
    /// Whether the user explicitly configured their identity
    configured: Arc<RwLock<bool>>,
    /// Tracks last header update time per URI to prevent infinite save loops
    last_header_update: Arc<RwLock<HashMap<Url, std::time::Instant>>>,
}

impl Backend {
    fn filename_from_uri(uri: &Url) -> String {
        let path = uri.path();
        let encoded = path.rsplit('/').next().unwrap_or("file");
        let mut decoded = String::with_capacity(encoded.len());
        let mut bytes = encoded.bytes();
        while let Some(b) = bytes.next() {
            if b == b'%' {
                let h = bytes.next().and_then(|c| (c as char).to_digit(16));
                let l = bytes.next().and_then(|c| (c as char).to_digit(16));
                if let (Some(h), Some(l)) = (h, l) {
                    decoded.push((h * 16 + l) as u8 as char);
                } else {
                    decoded.push('%');
                }
            } else {
                decoded.push(b as char);
            }
        }
        decoded
    }

    async fn get_username(&self) -> String {
        self.username.read().await.clone()
    }

    async fn get_email(&self) -> String {
        let email = self.email.read().await.clone();
        if email.is_empty() {
            let user = self.get_username().await;
            format!("{}@student.42.fr", user)
        } else {
            email
        }
    }

    async fn make_header_info(
        &self,
        filename: &str,
        existing: Option<&HeaderInfo>,
    ) -> HeaderInfo {
        let user = self.get_username().await;
        let mail = self.get_email().await;
        let now = Local::now();

        HeaderInfo {
            filename: filename.to_string(),
            author: format!("{} <{}>", user, mail),
            created_by: existing
                .map(|e| e.created_by.clone())
                .unwrap_or_else(|| user.clone()),
            created_at: existing.map(|e| e.created_at).unwrap_or(now),
            updated_by: user,
            updated_at: now,
        }
    }

    /// Show a setup guide notification if the user hasn't configured their identity
    async fn show_setup_guide_if_needed(&self) {
        if *self.configured.read().await {
            return;
        }

        let config_path = default_config_path();
        let msg = format!(
            "42 Header: Using system username. To set your 42 login, \
            create {}:\n\n\
            username = \"YOUR_LOGIN\"\n\
            email = \"YOUR_LOGIN@student.42.fr\"\n\n\
            Or add to Zed settings (Cmd+,):\n\n\
            \"lsp\": {{ \"header-42-lsp\": {{ \"initialization_options\": \
            {{ \"username\": \"YOUR_LOGIN\", \"email\": \"YOUR_LOGIN@student.42.fr\" }} }} }}",
            config_path.display()
        );

        self.client.show_message(MessageType::INFO, msg).await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // 1. Parse initialization_options from Zed settings
        let init_opts = params
            .initialization_options
            .and_then(|v| serde_json::from_value::<UserConfig>(v).ok())
            .unwrap_or_default();

        // 2. Load config file
        let file_cfg = load_config_file();

        // 3. Load environment variables
        let env_cfg = load_env_config();

        // Check if the user explicitly configured (init_opts or file)
        let explicitly_configured = init_opts.username.is_some() || file_cfg.username.is_some();

        // 4. Merge with priority
        let (username, email) = resolve_config(init_opts, file_cfg, env_cfg);

        *self.username.write().await = username;
        *self.email.write().await = email;
        *self.configured.write().await = explicitly_configured;

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(true),
                        })),
                        ..Default::default()
                    },
                )),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["42header.insertHeader".to_string()],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        let user = self.get_username().await;
        self.client
            .log_message(
                MessageType::INFO,
                format!("42 Header LSP initialized (user: {})", user),
            )
            .await;

        // Show setup guide if not explicitly configured
        self.show_setup_guide_if_needed().await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        self.documents.write().await.insert(
            doc.uri,
            DocumentState {
                content: doc.text,
                language_id: doc.language_id,
            },
        );
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            let mut docs = self.documents.write().await;
            if let Some(doc) = docs.get_mut(&params.text_document.uri) {
                doc.content = change.text;
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .write()
            .await
            .remove(&params.text_document.uri);
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;

        let (content, language_id) = {
            let docs = self.documents.read().await;
            match docs.get(&uri) {
                Some(doc) => {
                    let content = params.text.unwrap_or_else(|| doc.content.clone());
                    (content, doc.language_id.clone())
                }
                None => return,
            }
        };

        let header_text = match extract_header(&content) {
            Some(h) => h,
            None => return,
        };

        let info = get_header_info(&header_text);

        // Prevent infinite save loop using server-side state
        {
            let updates = self.last_header_update.read().await;
            if let Some(last) = updates.get(&uri) {
                if last.elapsed().as_secs() < 2 {
                    return;
                }
            }
        }

        let filename = Self::filename_from_uri(&uri);
        let new_info = self.make_header_info(&filename, Some(&info)).await;
        let new_header = render_header(&language_id, &filename, &new_info);

        let edit = WorkspaceEdit {
            changes: Some(HashMap::from([(
                uri.clone(),
                vec![TextEdit {
                    range: Range {
                        start: Position {
                            line: 0,
                            character: 0,
                        },
                        end: Position {
                            line: HEADER_END_LINE,
                            character: 0,
                        },
                    },
                    new_text: new_header,
                }],
            )])),
            ..Default::default()
        };

        let _ = self.client.apply_edit(edit).await;
        self.last_header_update
            .write()
            .await
            .insert(uri, std::time::Instant::now());
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        let docs = self.documents.read().await;
        let doc = match docs.get(uri) {
            Some(d) => d,
            None => return Ok(None),
        };

        let filename = Self::filename_from_uri(uri);
        let existing_header = extract_header(&doc.content);
        let has_header = existing_header.is_some();
        let title = if has_header {
            "Update 42 Header"
        } else {
            "Insert 42 Header"
        };

        let existing_info = existing_header.as_ref().map(|h| get_header_info(h));
        let new_info = self
            .make_header_info(&filename, existing_info.as_ref())
            .await;
        let new_header = render_header(&doc.language_id, &filename, &new_info);

        let edit_range = if has_header {
            Range {
                start: Position { line: 0, character: 0 },
                end: Position { line: HEADER_END_LINE, character: 0 },
            }
        } else {
            Range {
                start: Position { line: 0, character: 0 },
                end: Position { line: 0, character: 0 },
            }
        };

        let action = CodeAction {
            title: title.to_string(),
            kind: Some(CodeActionKind::SOURCE),
            edit: Some(WorkspaceEdit {
                changes: Some(HashMap::from([(
                    uri.clone(),
                    vec![TextEdit {
                        range: edit_range,
                        new_text: new_header,
                    }],
                )])),
                ..Default::default()
            }),
            ..Default::default()
        };

        Ok(Some(vec![CodeActionOrCommand::CodeAction(action)]))
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        if params.command == "42header.insertHeader" {
            if let Some(uri_value) = params.arguments.first() {
                if let Ok(uri_str) = serde_json::from_value::<String>(uri_value.clone()) {
                    if let Ok(uri) = Url::parse(&uri_str) {
                        let doc_data = {
                            let docs = self.documents.read().await;
                            docs.get(&uri)
                                .map(|doc| (doc.content.clone(), doc.language_id.clone()))
                        };

                        if let Some((content, language_id)) = doc_data {
                            let filename = Self::filename_from_uri(&uri);
                            let existing_header = extract_header(&content);
                            let existing_info =
                                existing_header.as_ref().map(|h| get_header_info(h));
                            let new_info = self
                                .make_header_info(&filename, existing_info.as_ref())
                                .await;
                            let new_header =
                                render_header(&language_id, &filename, &new_info);

                            let edit_range = if existing_header.is_some() {
                                Range {
                                    start: Position { line: 0, character: 0 },
                                    end: Position { line: HEADER_END_LINE, character: 0 },
                                }
                            } else {
                                Range {
                                    start: Position { line: 0, character: 0 },
                                    end: Position { line: 0, character: 0 },
                                }
                            };

                            let edit = WorkspaceEdit {
                                changes: Some(HashMap::from([(
                                    uri,
                                    vec![TextEdit {
                                        range: edit_range,
                                        new_text: new_header,
                                    }],
                                )])),
                                ..Default::default()
                            };

                            let _ = self.client.apply_edit(edit).await;
                        }
                    }
                }
            }
        }
        Ok(None)
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        documents: Arc::new(RwLock::new(HashMap::new())),
        username: Arc::new(RwLock::new("marvin".to_string())),
        email: Arc::new(RwLock::new(String::new())),
        configured: Arc::new(RwLock::new(false)),
        last_header_update: Arc::new(RwLock::new(HashMap::new())),
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
