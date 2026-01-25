use crate::compiler::ast::{Import, Item, Program};
use crate::compiler::lexer::Lexer;
use crate::compiler::parser::Parser;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A module loader that resolves import paths and loads module files.
pub struct ModuleLoader {
    /// Root directory for the project
    root_dir: PathBuf,
    /// Cache of loaded modules (path -> parsed program)
    cache: HashMap<PathBuf, Program>,
    /// Search paths for modules
    search_paths: Vec<PathBuf>,
}

impl ModuleLoader {
    pub fn new(root_dir: PathBuf) -> Self {
        let mut search_paths = vec![];

        // Add src directory to search path
        let src_dir = root_dir.join("src");
        if src_dir.exists() {
            search_paths.push(src_dir);
        }

        // Add root directory itself
        search_paths.push(root_dir.clone());

        Self {
            root_dir,
            cache: HashMap::new(),
            search_paths,
        }
    }

    /// Resolve an import to a file path.
    pub fn resolve_import(&self, import: &Import, from_file: &Path) -> Result<PathBuf, String> {
        if import.relative {
            // Relative import: resolve from the importing file's directory
            let base_dir = from_file.parent().unwrap_or(Path::new("."));
            let module_name = import.path.join("/");
            let module_path = base_dir.join(format!("{}.mica", module_name));

            if module_path.exists() {
                Ok(module_path)
            } else {
                Err(format!(
                    "module '{}' not found (looked at {})",
                    import.path.join("."),
                    module_path.display()
                ))
            }
        } else {
            // Absolute import: search in search paths
            self.resolve_absolute_import(&import.path)
        }
    }

    /// Resolve an absolute import path.
    fn resolve_absolute_import(&self, path: &[String]) -> Result<PathBuf, String> {
        // Convert module path to file path
        // import utils.http -> utils/http.mica or utils/http/mod.mica
        let file_path = format!("{}.mica", path.join("/"));
        let mod_path = format!("{}/mod.mica", path.join("/"));

        for search_path in &self.search_paths {
            // Try direct file
            let full_path = search_path.join(&file_path);
            if full_path.exists() {
                return Ok(full_path);
            }

            // Try mod.mica in directory
            let full_mod_path = search_path.join(&mod_path);
            if full_mod_path.exists() {
                return Ok(full_mod_path);
            }
        }

        Err(format!(
            "module '{}' not found in search paths: {:?}",
            path.join("."),
            self.search_paths
        ))
    }

    /// Load a module from a file path.
    pub fn load_module(&mut self, path: &Path) -> Result<&Program, String> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        if self.cache.contains_key(&canonical) {
            return Ok(self.cache.get(&canonical).unwrap());
        }

        // Read and parse the file
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read module '{}': {}", path.display(), e))?;

        let filename = path.to_string_lossy().to_string();
        let mut lexer = Lexer::new(&filename, &source);
        let tokens = lexer.scan_tokens()?;
        let mut parser = Parser::new(&filename, tokens);
        let program = parser.parse()?;

        self.cache.insert(canonical.clone(), program);
        Ok(self.cache.get(&canonical).unwrap())
    }

    /// Load all imports for a program and return a combined program.
    pub fn load_with_imports(&mut self, main_path: &Path) -> Result<Program, String> {
        let main_program = {
            let source = std::fs::read_to_string(main_path)
                .map_err(|e| format!("failed to read '{}': {}", main_path.display(), e))?;

            let filename = main_path.to_string_lossy().to_string();
            let mut lexer = Lexer::new(&filename, &source);
            let tokens = lexer.scan_tokens()?;
            let mut parser = Parser::new(&filename, tokens);
            parser.parse()?
        };

        // Collect imports
        let imports: Vec<_> = main_program.items.iter()
            .filter_map(|item| {
                if let Item::Import(import) = item {
                    Some(import.clone())
                } else {
                    None
                }
            })
            .collect();

        // Load imported modules and collect their items
        let mut all_items = Vec::new();

        for import in imports {
            let module_path = self.resolve_import(&import, main_path)?;
            let module = self.load_module(&module_path)?;

            // Add non-import items from the module
            for item in &module.items {
                match item {
                    Item::Import(_) => {
                        // TODO: Handle transitive imports
                    }
                    Item::FnDef(fn_def) => {
                        all_items.push(Item::FnDef(fn_def.clone()));
                    }
                    Item::Statement(_) => {
                        // Module-level statements are not imported
                    }
                }
            }
        }

        // Add main program items (excluding imports)
        for item in main_program.items {
            match item {
                Item::Import(_) => {
                    // Already processed
                }
                other => {
                    all_items.push(other);
                }
            }
        }

        Ok(Program { items: all_items })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::fs;

    #[test]
    fn test_resolve_absolute_import() {
        let temp = temp_dir().join("mica_module_test");
        if temp.exists() {
            fs::remove_dir_all(&temp).unwrap();
        }
        fs::create_dir_all(&temp.join("src")).unwrap();

        // Create a module file
        fs::write(temp.join("src/utils.mica"), "fun helper() { return 42; }").unwrap();

        let loader = ModuleLoader::new(temp.clone());

        let import = Import {
            path: vec!["utils".to_string()],
            relative: false,
            span: crate::compiler::lexer::Span { line: 1, column: 1 },
        };

        let result = loader.resolve_import(&import, &temp.join("src/main.mica"));
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("utils.mica"));

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn test_resolve_relative_import() {
        let temp = temp_dir().join("mica_module_test_rel");
        if temp.exists() {
            fs::remove_dir_all(&temp).unwrap();
        }
        fs::create_dir_all(&temp.join("src")).unwrap();

        // Create a relative module file
        fs::write(temp.join("src/local.mica"), "fun local_fn() { return 1; }").unwrap();

        let loader = ModuleLoader::new(temp.clone());

        let import = Import {
            path: vec!["local".to_string()],
            relative: true,
            span: crate::compiler::lexer::Span { line: 1, column: 1 },
        };

        let result = loader.resolve_import(&import, &temp.join("src/main.mica"));
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("local.mica"));

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn test_load_with_imports() {
        let temp = temp_dir().join("mica_module_test_load");
        if temp.exists() {
            fs::remove_dir_all(&temp).unwrap();
        }
        fs::create_dir_all(&temp.join("src")).unwrap();

        // Create main file with import
        fs::write(
            temp.join("src/main.mica"),
            "import utils;\nlet x = helper();\nprint(x);",
        ).unwrap();

        // Create utils module
        fs::write(
            temp.join("src/utils.mica"),
            "fun helper() { return 42; }",
        ).unwrap();

        let mut loader = ModuleLoader::new(temp.clone());
        let program = loader.load_with_imports(&temp.join("src/main.mica")).unwrap();

        // Should have: helper function + 2 statements from main
        let fn_count = program.items.iter().filter(|i| matches!(i, Item::FnDef(_))).count();
        let stmt_count = program.items.iter().filter(|i| matches!(i, Item::Statement(_))).count();

        assert_eq!(fn_count, 1);
        assert_eq!(stmt_count, 2);

        fs::remove_dir_all(&temp).ok();
    }
}
