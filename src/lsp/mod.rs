use std::collections::HashMap;
use std::sync::RwLock;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::compiler::ast::Program;
use crate::compiler::{Lexer, Parser, Resolver};

mod symbols;
use symbols::SymbolTable;

/// The mica language server backend.
pub struct MicaLanguageServer {
    client: Client,
    /// Document cache: URI -> source text
    documents: RwLock<HashMap<Url, String>>,
}

impl MicaLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: RwLock::new(HashMap::new()),
        }
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
                source: Some("mica".to_string()),
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
        source: Some("mica".to_string()),
        message: message.to_string(),
        ..Default::default()
    })
}

#[tower_lsp::async_trait]
impl LanguageServer for MicaLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
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
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "mica-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "mica language server initialized")
            .await;
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

        let builtins = [
            "print",
            "len",
            "push",
            "pop",
            "type_of",
            "to_string",
            "parse_int",
        ];

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
}

/// Run the LSP server on stdin/stdout.
pub async fn run_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(MicaLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
