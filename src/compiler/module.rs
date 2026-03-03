use crate::compiler::ast::{Import, Item, Program};
use crate::compiler::lexer::Lexer;
use crate::compiler::parser::Parser;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Timing information from loading modules
#[derive(Debug, Clone, Default)]
pub struct LoadTimings {
    pub lexer: Duration,
    pub parser: Duration,
}

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
            let module_path = base_dir.join(format!("{}.mc", module_name));

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
        // import utils.http -> utils/http.mc or utils/http/mod.mc
        let file_path = format!("{}.mc", path.join("/"));
        let mod_path = format!("{}/mod.mc", path.join("/"));

        for search_path in &self.search_paths {
            // Try direct file
            let full_path = search_path.join(&file_path);
            if full_path.exists() {
                return Ok(full_path);
            }

            // Try mod.mc in directory
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
        self.load_module_timed(path, None)
    }

    /// Load a module from a file path, optionally tracking timing.
    pub fn load_module_timed(
        &mut self,
        path: &Path,
        mut timings: Option<&mut LoadTimings>,
    ) -> Result<&Program, String> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        if self.cache.contains_key(&canonical) {
            return Ok(self.cache.get(&canonical).unwrap());
        }

        // Read and parse the file
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read module '{}': {}", path.display(), e))?;

        let filename = path.to_string_lossy().to_string();

        let start = Instant::now();
        let mut lexer = Lexer::new(&filename, &source);
        let tokens = lexer.scan_tokens()?;
        if let Some(ref mut t) = timings {
            t.lexer += start.elapsed();
        }

        let start = Instant::now();
        let mut parser = Parser::new(&filename, tokens);
        let program = parser.parse()?;
        if let Some(t) = timings {
            t.parser += start.elapsed();
        }

        self.cache.insert(canonical.clone(), program);
        Ok(self.cache.get(&canonical).unwrap())
    }

    /// Load all imports for a program and return a combined program.
    pub fn load_with_imports(&mut self, main_path: &Path) -> Result<Program, String> {
        self.load_with_imports_timed(main_path, None)
            .map(|(program, _)| program)
    }

    /// Load all imports for a program and return a combined program with timing info.
    pub fn load_with_imports_timed(
        &mut self,
        main_path: &Path,
        timings: Option<&mut LoadTimings>,
    ) -> Result<(Program, LoadTimings), String> {
        let mut load_timings = LoadTimings::default();

        let main_program = {
            let source = std::fs::read_to_string(main_path)
                .map_err(|e| format!("failed to read '{}': {}", main_path.display(), e))?;

            let filename = main_path.to_string_lossy().to_string();

            let start = Instant::now();
            let mut lexer = Lexer::new(&filename, &source);
            let tokens = lexer.scan_tokens()?;
            load_timings.lexer += start.elapsed();

            let start = Instant::now();
            let mut parser = Parser::new(&filename, tokens);
            let program = parser.parse()?;
            load_timings.parser += start.elapsed();

            program
        };

        // Collect imports
        let imports: Vec<_> = main_program
            .items
            .iter()
            .filter_map(|item| {
                if let Item::Import(import) = item {
                    Some(import.clone())
                } else {
                    None
                }
            })
            .collect();

        // Load imported modules and collect their items (with transitive imports)
        let mut all_items = Vec::new();
        let main_canonical = main_path
            .canonicalize()
            .unwrap_or_else(|_| main_path.to_path_buf());
        let mut fully_loaded = HashSet::new();
        let mut in_progress = HashSet::new();
        in_progress.insert(main_canonical);

        for import in imports {
            let module_path = self.resolve_import(&import, main_path)?;
            self.collect_module_items(
                &module_path,
                &mut all_items,
                &mut fully_loaded,
                &mut in_progress,
                &mut load_timings,
            )?;
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

        // Update external timings if provided
        if let Some(t) = timings {
            t.lexer += load_timings.lexer;
            t.parser += load_timings.parser;
        }

        Ok((Program { items: all_items }, load_timings))
    }

    /// Recursively collect items from a module and its transitive imports.
    ///
    /// `fully_loaded` tracks modules that have been completely processed (for deduplication).
    /// `in_progress` tracks modules currently being processed (for circular import detection).
    fn collect_module_items(
        &mut self,
        module_path: &Path,
        all_items: &mut Vec<Item>,
        fully_loaded: &mut HashSet<PathBuf>,
        in_progress: &mut HashSet<PathBuf>,
        load_timings: &mut LoadTimings,
    ) -> Result<(), String> {
        let canonical = module_path
            .canonicalize()
            .unwrap_or_else(|_| module_path.to_path_buf());

        // Skip already fully-loaded modules (diamond dependency deduplication)
        if fully_loaded.contains(&canonical) {
            return Ok(());
        }

        // Detect circular imports: module is currently being processed
        if in_progress.contains(&canonical) {
            return Err(format!(
                "circular import detected: '{}' is already being imported",
                module_path.display()
            ));
        }

        in_progress.insert(canonical.clone());

        let module = self.load_module_timed(module_path, Some(load_timings))?;

        // Clone items to avoid borrow issues
        let items: Vec<Item> = module.items.clone();

        // First, recursively load transitive imports
        for item in &items {
            if let Item::Import(import) = item {
                let transitive_path = self.resolve_import(import, module_path)?;
                self.collect_module_items(
                    &transitive_path,
                    all_items,
                    fully_loaded,
                    in_progress,
                    load_timings,
                )?;
            }
        }

        // Then, add non-import items from this module
        for item in items {
            match item {
                Item::Import(_) => {
                    // Already processed above
                }
                Item::Statement(_) => {
                    // Module-level statements are not imported
                }
                other => {
                    all_items.push(other);
                }
            }
        }

        // Mark as fully loaded and remove from in-progress
        in_progress.remove(&canonical);
        fully_loaded.insert(canonical);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::fs;

    #[test]
    fn test_resolve_absolute_import() {
        let temp = temp_dir().join("moca_module_test");
        if temp.exists() {
            fs::remove_dir_all(&temp).unwrap();
        }
        fs::create_dir_all(&temp.join("src")).unwrap();

        // Create a module file
        fs::write(temp.join("src/utils.mc"), "fun helper() { return 42; }").unwrap();

        let loader = ModuleLoader::new(temp.clone());

        let import = Import {
            path: vec!["utils".to_string()],
            relative: false,
            span: crate::compiler::lexer::Span { line: 1, column: 1 },
        };

        let result = loader.resolve_import(&import, &temp.join("src/main.mc"));
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("utils.mc"));

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn test_resolve_relative_import() {
        let temp = temp_dir().join("moca_module_test_rel");
        if temp.exists() {
            fs::remove_dir_all(&temp).unwrap();
        }
        fs::create_dir_all(&temp.join("src")).unwrap();

        // Create a relative module file
        fs::write(temp.join("src/local.mc"), "fun local_fn() { return 1; }").unwrap();

        let loader = ModuleLoader::new(temp.clone());

        let import = Import {
            path: vec!["local".to_string()],
            relative: true,
            span: crate::compiler::lexer::Span { line: 1, column: 1 },
        };

        let result = loader.resolve_import(&import, &temp.join("src/main.mc"));
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("local.mc"));

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn test_load_with_imports() {
        let temp = temp_dir().join("moca_module_test_load");
        if temp.exists() {
            fs::remove_dir_all(&temp).unwrap();
        }
        fs::create_dir_all(&temp.join("src")).unwrap();

        // Create main file with import
        fs::write(
            temp.join("src/main.mc"),
            "import utils;\nlet x = helper();\nprint(x);",
        )
        .unwrap();

        // Create utils module
        fs::write(temp.join("src/utils.mc"), "fun helper() { return 42; }").unwrap();

        let mut loader = ModuleLoader::new(temp.clone());
        let program = loader.load_with_imports(&temp.join("src/main.mc")).unwrap();

        // Should have: helper function + 2 statements from main
        let fn_count = program
            .items
            .iter()
            .filter(|i| matches!(i, Item::FnDef(_)))
            .count();
        let stmt_count = program
            .items
            .iter()
            .filter(|i| matches!(i, Item::Statement(_)))
            .count();

        assert_eq!(fn_count, 1);
        assert_eq!(stmt_count, 2);

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn test_transitive_imports() {
        let temp = temp_dir().join("moca_module_test_transitive");
        if temp.exists() {
            fs::remove_dir_all(&temp).unwrap();
        }
        fs::create_dir_all(&temp.join("src")).unwrap();

        // main imports a, a imports b
        fs::write(
            temp.join("src/main.mc"),
            "import a;\nlet x = fn_a();\nprint(x);",
        )
        .unwrap();
        fs::write(
            temp.join("src/a.mc"),
            "import b;\nfun fn_a() { return fn_b(); }",
        )
        .unwrap();
        fs::write(temp.join("src/b.mc"), "fun fn_b() { return 42; }").unwrap();

        let mut loader = ModuleLoader::new(temp.clone());
        let program = loader.load_with_imports(&temp.join("src/main.mc")).unwrap();

        // Should have: fn_b (from b, loaded transitively) + fn_a (from a) + 2 statements from main
        let fn_count = program
            .items
            .iter()
            .filter(|i| matches!(i, Item::FnDef(_)))
            .count();
        assert_eq!(fn_count, 2);

        // fn_b should come before fn_a (dependencies loaded first)
        let fn_names: Vec<&str> = program
            .items
            .iter()
            .filter_map(|i| {
                if let Item::FnDef(f) = i {
                    Some(f.name.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(fn_names, vec!["fn_b", "fn_a"]);

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn test_diamond_imports() {
        let temp = temp_dir().join("moca_module_test_diamond");
        if temp.exists() {
            fs::remove_dir_all(&temp).unwrap();
        }
        fs::create_dir_all(&temp.join("src")).unwrap();

        // main imports a and b; both a and b import shared
        fs::write(
            temp.join("src/main.mc"),
            "import a;\nimport b;\nlet x = fn_a();\nlet y = fn_b();",
        )
        .unwrap();
        fs::write(
            temp.join("src/a.mc"),
            "import shared;\nfun fn_a() { return shared_fn(); }",
        )
        .unwrap();
        fs::write(
            temp.join("src/b.mc"),
            "import shared;\nfun fn_b() { return shared_fn(); }",
        )
        .unwrap();
        fs::write(temp.join("src/shared.mc"), "fun shared_fn() { return 1; }").unwrap();

        let mut loader = ModuleLoader::new(temp.clone());
        let program = loader.load_with_imports(&temp.join("src/main.mc")).unwrap();

        // shared_fn should appear exactly once (deduplication)
        let fn_names: Vec<&str> = program
            .items
            .iter()
            .filter_map(|i| {
                if let Item::FnDef(f) = i {
                    Some(f.name.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(fn_names, vec!["shared_fn", "fn_a", "fn_b"]);

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn test_circular_import_detection() {
        let temp = temp_dir().join("moca_module_test_circular");
        if temp.exists() {
            fs::remove_dir_all(&temp).unwrap();
        }
        fs::create_dir_all(&temp.join("src")).unwrap();

        // main imports a, a imports b, b imports a (circular)
        fs::write(temp.join("src/main.mc"), "import a;\nprint(1);").unwrap();
        fs::write(temp.join("src/a.mc"), "import b;\nfun fn_a() { return 1; }").unwrap();
        fs::write(temp.join("src/b.mc"), "import a;\nfun fn_b() { return 2; }").unwrap();

        let mut loader = ModuleLoader::new(temp.clone());
        let result = loader.load_with_imports(&temp.join("src/main.mc"));

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("circular import detected"),
            "expected circular import error, got: {}",
            err
        );

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn test_self_import_detection() {
        let temp = temp_dir().join("moca_module_test_self_import");
        if temp.exists() {
            fs::remove_dir_all(&temp).unwrap();
        }
        fs::create_dir_all(&temp.join("src")).unwrap();

        // main imports a, a imports itself
        fs::write(temp.join("src/main.mc"), "import a;\nprint(1);").unwrap();
        fs::write(temp.join("src/a.mc"), "import a;\nfun fn_a() { return 1; }").unwrap();

        let mut loader = ModuleLoader::new(temp.clone());
        let result = loader.load_with_imports(&temp.join("src/main.mc"));

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("circular import detected"),
            "expected circular import error, got: {}",
            err
        );

        fs::remove_dir_all(&temp).ok();
    }
}
