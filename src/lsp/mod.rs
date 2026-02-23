use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::compiler::ast::Program;
use crate::compiler::{Lexer, Parser, Resolver};

mod symbols;
use symbols::{DocSymbol, SymbolTable};

/// The moca language server backend.
pub struct MocaLanguageServer {
    client: Client,
    /// Document cache: URI -> source text
    documents: RwLock<HashMap<Url, String>>,
    /// Workspace root path
    workspace_root: RwLock<Option<PathBuf>>,
}

impl MocaLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: RwLock::new(HashMap::new()),
            workspace_root: RwLock::new(None),
        }
    }

    /// Scan workspace for .mc files and index them.
    fn scan_workspace(&self, root: &Path) {
        let mc_files = Self::find_mc_files(root);
        let mut docs = self.documents.write().unwrap();

        for path in mc_files {
            if let (Ok(source), Ok(uri)) =
                (std::fs::read_to_string(&path), Url::from_file_path(&path))
            {
                docs.entry(uri).or_insert(source);
            }
        }
    }

    /// Recursively find all .mc files under a directory.
    fn find_mc_files(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return files;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip hidden dirs and common non-source dirs
                if path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| {
                        name.starts_with('.') || name == "target" || name == "node_modules"
                    })
                {
                    continue;
                }
                files.extend(Self::find_mc_files(&path));
            } else if path.extension().is_some_and(|ext| ext == "mc") {
                files.push(path);
            }
        }
        files
    }

    /// Parse a document and return the AST (if successful).
    fn parse_document(&self, uri: &Url, source: &str) -> Option<Program> {
        let filename = uri.path();

        // Try lexing
        let mut lexer = Lexer::new(filename, source);
        let tokens = lexer.scan_tokens().ok()?;

        // Try parsing
        let mut parser = Parser::new(filename, tokens);
        parser.parse().ok()
    }

    /// Analyze a document and return diagnostics.
    fn analyze(&self, uri: &Url, source: &str) -> Vec<Diagnostic> {
        let filename = uri.path();
        let mut diagnostics = Vec::new();

        // Try lexing
        let mut lexer = Lexer::new(filename, source);
        let tokens = match lexer.scan_tokens() {
            Ok(tokens) => tokens,
            Err(e) => {
                if let Some(diag) = parse_error_to_diagnostic(&e) {
                    diagnostics.push(diag);
                }
                return diagnostics;
            }
        };

        // Try parsing
        let mut parser = Parser::new(filename, tokens);
        let program = match parser.parse() {
            Ok(program) => program,
            Err(e) => {
                if let Some(diag) = parse_error_to_diagnostic(&e) {
                    diagnostics.push(diag);
                }
                return diagnostics;
            }
        };

        // Try resolving
        let mut resolver = Resolver::new(filename);
        if let Err(e) = resolver.resolve(program)
            && let Some(diag) = parse_error_to_diagnostic(&e)
        {
            diagnostics.push(diag);
        }

        diagnostics
    }
}

/// Parse an error message to extract location and create a diagnostic.
fn parse_error_to_diagnostic(error: &str) -> Option<Diagnostic> {
    // Error format: "error: MESSAGE\n  --> FILE:LINE:COLUMN"
    let lines: Vec<&str> = error.lines().collect();

    let message = lines
        .first()
        .and_then(|l| l.strip_prefix("error: "))
        .unwrap_or(error);

    // Try to extract location from second line
    if let Some(location_line) = lines.get(1)
        && let Some(loc) = location_line.strip_prefix("  --> ")
    {
        let parts: Vec<&str> = loc.split(':').collect();
        if parts.len() >= 3
            && let (Ok(line), Ok(col)) = (
                parts[parts.len() - 2].parse::<u32>(),
                parts[parts.len() - 1].parse::<u32>(),
            )
        {
            let line = line.saturating_sub(1);
            let col = col.saturating_sub(1);
            return Some(Diagnostic {
                range: Range {
                    start: Position {
                        line,
                        character: col,
                    },
                    end: Position {
                        line,
                        character: col + 1,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("moca".to_string()),
                message: message.to_string(),
                ..Default::default()
            });
        }
    }

    // Fallback: return diagnostic at start of file
    Some(Diagnostic {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 1,
            },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("moca".to_string()),
        message: message.to_string(),
        ..Default::default()
    })
}

#[tower_lsp::async_trait]
impl LanguageServer for MocaLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Capture workspace root from rootUri or workspaceFolders
        let root_path = params
            .root_uri
            .as_ref()
            .and_then(|uri| uri.to_file_path().ok())
            .or_else(|| {
                params
                    .workspace_folders
                    .as_ref()
                    .and_then(|folders| folders.first().and_then(|f| f.uri.to_file_path().ok()))
            });

        if let Some(root) = root_path {
            let mut wr = self.workspace_root.write().unwrap();
            *wr = Some(root);
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string()]),
                    ..Default::default()
                }),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "moca-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        // Scan workspace for .mc files
        let root = {
            let wr = self.workspace_root.read().unwrap();
            wr.clone()
        };

        if let Some(root) = root {
            self.scan_workspace(&root);
            let doc_count = self.documents.read().unwrap().len();
            self.client
                .log_message(
                    MessageType::INFO,
                    format!(
                        "moca language server initialized — indexed {} .mc files",
                        doc_count
                    ),
                )
                .await;
        } else {
            self.client
                .log_message(MessageType::INFO, "moca language server initialized")
                .await;
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;

        {
            let mut docs = self.documents.write().unwrap();
            docs.insert(uri.clone(), text.clone());
        }

        let diagnostics = self.analyze(&uri, &text);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;

        // We use FULL sync, so we get the entire document
        if let Some(change) = params.content_changes.into_iter().next() {
            let text = change.text;

            {
                let mut docs = self.documents.write().unwrap();
                docs.insert(uri.clone(), text.clone());
            }

            let diagnostics = self.analyze(&uri, &text);
            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;

        {
            let mut docs = self.documents.write().unwrap();
            docs.remove(&uri);
        }

        // Clear diagnostics when file is closed
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;

        let source = {
            let docs = self.documents.read().unwrap();
            match docs.get(uri) {
                Some(s) => s.clone(),
                None => return Ok(None),
            }
        };

        // Basic keyword completion
        let keywords = vec![
            "let", "var", "fun", "if", "else", "while", "for", "in", "return", "true", "false",
            "nil", "try", "catch", "throw", "import",
        ];

        let builtins = ["print", "len", "push", "pop", "type_of", "to_string"];

        let mut items: Vec<CompletionItem> = keywords
            .iter()
            .map(|&kw| CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            })
            .collect();

        items.extend(builtins.iter().map(|&b| CompletionItem {
            label: b.to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            ..Default::default()
        }));

        // Add symbols from the document
        if let Some(program) = self.parse_document(uri, &source) {
            let symbols = SymbolTable::from_program(&program);
            for (name, defs) in &symbols.definitions {
                if let Some(def) = defs.first() {
                    let kind = match def.kind {
                        symbols::SymbolKind::Function => CompletionItemKind::FUNCTION,
                        symbols::SymbolKind::Variable => CompletionItemKind::VARIABLE,
                        symbols::SymbolKind::Parameter => CompletionItemKind::VARIABLE,
                        symbols::SymbolKind::Struct => CompletionItemKind::STRUCT,
                        symbols::SymbolKind::Interface => CompletionItemKind::INTERFACE,
                        symbols::SymbolKind::Method => CompletionItemKind::METHOD,
                        symbols::SymbolKind::Field => CompletionItemKind::FIELD,
                    };
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: Some(kind),
                        ..Default::default()
                    });
                }
            }
        }

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let source = {
            let docs = self.documents.read().unwrap();
            match docs.get(uri) {
                Some(s) => s.clone(),
                None => return Ok(None),
            }
        };

        // Parse the document
        let program = match self.parse_document(uri, &source) {
            Some(p) => p,
            None => return Ok(None),
        };

        // Build symbol table
        let symbols = SymbolTable::from_program(&program);

        // Find symbol at cursor position (convert to 1-based)
        let line = position.line + 1;
        let column = position.character + 1;

        let symbol_name = match symbols.find_at_position(line, column) {
            Some(name) => name,
            None => return Ok(None),
        };

        // Find definition
        let def = match symbols.get_definition(symbol_name) {
            Some(d) => d,
            None => return Ok(None),
        };

        // Convert span to LSP position (0-based)
        let def_line = (def.def_span.line.saturating_sub(1)) as u32;
        let def_col = (def.def_span.column.saturating_sub(1)) as u32;

        let location = Location {
            uri: uri.clone(),
            range: Range {
                start: Position {
                    line: def_line,
                    character: def_col,
                },
                end: Position {
                    line: def_line,
                    character: def_col + def.name.len() as u32,
                },
            },
        };

        Ok(Some(GotoDefinitionResponse::Scalar(location)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let source = {
            let docs = self.documents.read().unwrap();
            match docs.get(uri) {
                Some(s) => s.clone(),
                None => return Ok(None),
            }
        };

        // Parse the document
        let program = match self.parse_document(uri, &source) {
            Some(p) => p,
            None => return Ok(None),
        };

        // Build symbol table
        let symbols = SymbolTable::from_program(&program);

        // Find symbol at cursor position (convert to 1-based)
        let line = position.line + 1;
        let column = position.character + 1;

        let symbol_name = match symbols.find_at_position(line, column) {
            Some(name) => name,
            None => return Ok(None),
        };

        // Find definition to get kind
        let kind_str = match symbols.get_definition(symbol_name) {
            Some(def) => match def.kind {
                symbols::SymbolKind::Function => "function",
                symbols::SymbolKind::Variable => "variable",
                symbols::SymbolKind::Parameter => "parameter",
                symbols::SymbolKind::Struct => "struct",
                symbols::SymbolKind::Interface => "interface",
                symbols::SymbolKind::Method => "method",
                symbols::SymbolKind::Field => "field",
            },
            None => "symbol",
        };

        let contents = HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("**{}** `{}`", kind_str, symbol_name),
        });

        Ok(Some(Hover {
            contents,
            range: None,
        }))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;

        let source = {
            let docs = self.documents.read().unwrap();
            match docs.get(uri) {
                Some(s) => s.clone(),
                None => return Ok(None),
            }
        };

        let program = match self.parse_document(uri, &source) {
            Some(p) => p,
            None => return Ok(None),
        };

        let symbols = SymbolTable::from_program(&program);
        let lsp_symbols: Vec<DocumentSymbol> =
            symbols.doc_symbols.iter().map(doc_symbol_to_lsp).collect();

        Ok(Some(DocumentSymbolResponse::Nested(lsp_symbols)))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let source = {
            let docs = self.documents.read().unwrap();
            match docs.get(uri) {
                Some(s) => s.clone(),
                None => return Ok(None),
            }
        };

        let program = match self.parse_document(uri, &source) {
            Some(p) => p,
            None => return Ok(None),
        };

        let symbols = SymbolTable::from_program(&program);

        let line = position.line + 1;
        let column = position.character + 1;

        let symbol_name = match symbols.find_at_position(line, column) {
            Some(name) => name.to_string(),
            None => return Ok(None),
        };

        let spans = symbols.find_references(&symbol_name);
        if spans.is_empty() {
            return Ok(None);
        }

        let locations: Vec<Location> = spans
            .iter()
            .map(|span| Location {
                uri: uri.clone(),
                range: span_to_range(span, symbol_name.len()),
            })
            .collect();

        Ok(Some(locations))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let query = params.query.to_lowercase();

        let docs = self.documents.read().unwrap();
        let mut results = Vec::new();

        for (uri, source) in docs.iter() {
            let program = match self.parse_document(uri, source) {
                Some(p) => p,
                None => continue,
            };

            let symbols = SymbolTable::from_program(&program);

            for sym in &symbols.doc_symbols {
                Self::collect_workspace_symbols(sym, uri, &query, &mut results);
            }
        }

        if results.is_empty() {
            Ok(None)
        } else {
            Ok(Some(results))
        }
    }

    async fn goto_implementation(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let source = {
            let docs = self.documents.read().unwrap();
            match docs.get(uri) {
                Some(s) => s.clone(),
                None => return Ok(None),
            }
        };

        let program = match self.parse_document(uri, &source) {
            Some(p) => p,
            None => return Ok(None),
        };

        let symbols = SymbolTable::from_program(&program);

        let line = position.line + 1;
        let column = position.character + 1;

        let symbol_name = match symbols.find_at_position(line, column) {
            Some(name) => name.to_string(),
            None => return Ok(None),
        };

        // Check if this is an interface — find its implementations
        if let Some(def) = symbols.get_definition(&symbol_name)
            && def.kind == symbols::SymbolKind::Interface
        {
            let impls = symbols.find_implementations(&symbol_name);
            if impls.is_empty() {
                return Ok(None);
            }
            let locations: Vec<Location> = impls
                .iter()
                .map(|impl_info| Location {
                    uri: uri.clone(),
                    range: span_to_range(&impl_info.span, impl_info.struct_name.len()),
                })
                .collect();

            return Ok(Some(GotoDefinitionResponse::Array(locations)));
        }

        // Check if this is a method call — find the impl method
        for impl_info in &symbols.impl_blocks {
            for method in &impl_info.methods {
                // method.name is "StructName.method_name"
                if method.name.ends_with(&format!(".{}", symbol_name)) {
                    let location = Location {
                        uri: uri.clone(),
                        range: span_to_range(&method.def_span, symbol_name.len()),
                    };
                    return Ok(Some(GotoDefinitionResponse::Scalar(location)));
                }
            }
        }

        Ok(None)
    }
}

impl MocaLanguageServer {
    /// Recursively collect workspace symbols matching a query.
    #[allow(deprecated)]
    fn collect_workspace_symbols(
        sym: &DocSymbol,
        uri: &Url,
        query: &str,
        results: &mut Vec<SymbolInformation>,
    ) {
        if query.is_empty() || sym.name.to_lowercase().contains(query) {
            let kind = match sym.kind {
                symbols::SymbolKind::Function => SymbolKind::FUNCTION,
                symbols::SymbolKind::Variable => SymbolKind::VARIABLE,
                symbols::SymbolKind::Parameter => SymbolKind::VARIABLE,
                symbols::SymbolKind::Struct => SymbolKind::STRUCT,
                symbols::SymbolKind::Interface => SymbolKind::INTERFACE,
                symbols::SymbolKind::Method => SymbolKind::METHOD,
                symbols::SymbolKind::Field => SymbolKind::FIELD,
            };

            results.push(SymbolInformation {
                name: sym.name.clone(),
                kind,
                tags: None,
                deprecated: None,
                location: Location {
                    uri: uri.clone(),
                    range: span_to_range(&sym.span, sym.name.len()),
                },
                container_name: None,
            });
        }

        for child in &sym.children {
            Self::collect_workspace_symbols(child, uri, query, results);
        }
    }
}

/// Convert a Span (1-based) to an LSP Range (0-based).
fn span_to_range(span: &crate::compiler::lexer::Span, name_len: usize) -> Range {
    let line = (span.line.saturating_sub(1)) as u32;
    let col = (span.column.saturating_sub(1)) as u32;
    Range {
        start: Position {
            line,
            character: col,
        },
        end: Position {
            line,
            character: col + name_len as u32,
        },
    }
}

/// Convert a DocSymbol to an LSP DocumentSymbol.
#[allow(deprecated)]
fn doc_symbol_to_lsp(sym: &DocSymbol) -> DocumentSymbol {
    let kind = match sym.kind {
        symbols::SymbolKind::Function => SymbolKind::FUNCTION,
        symbols::SymbolKind::Variable => SymbolKind::VARIABLE,
        symbols::SymbolKind::Parameter => SymbolKind::VARIABLE,
        symbols::SymbolKind::Struct => SymbolKind::STRUCT,
        symbols::SymbolKind::Interface => SymbolKind::INTERFACE,
        symbols::SymbolKind::Method => SymbolKind::METHOD,
        symbols::SymbolKind::Field => SymbolKind::FIELD,
    };

    let range = span_to_range(&sym.span, sym.name.len());
    let children = if sym.children.is_empty() {
        None
    } else {
        Some(sym.children.iter().map(doc_symbol_to_lsp).collect())
    };

    DocumentSymbol {
        name: sym.name.clone(),
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range: range,
        children,
    }
}

/// Run the LSP server on stdin/stdout.
pub async fn run_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(MocaLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
