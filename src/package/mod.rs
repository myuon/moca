use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Package manifest (pkg.toml)
#[derive(Debug, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: PackageInfo,
    #[serde(default)]
    pub dependencies: HashMap<String, Dependency>,
    #[serde(default, rename = "dev-dependencies")]
    pub dev_dependencies: HashMap<String, Dependency>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    #[serde(default = "default_entry")]
    pub entry: String,
}

fn default_entry() -> String {
    "src/main.mica".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Dependency {
    pub git: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

impl PackageManifest {
    /// Create a new package manifest with default values
    pub fn new(name: &str) -> Self {
        Self {
            package: PackageInfo {
                name: name.to_string(),
                version: "0.1.0".to_string(),
                entry: "src/main.mica".to_string(),
            },
            dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
        }
    }

    /// Load manifest from a directory
    pub fn load(dir: &Path) -> Result<Self, String> {
        let manifest_path = dir.join("pkg.toml");
        let content = fs::read_to_string(&manifest_path)
            .map_err(|e| format!("failed to read pkg.toml: {}", e))?;
        toml::from_str(&content).map_err(|e| format!("failed to parse pkg.toml: {}", e))
    }

    /// Save manifest to a directory
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let manifest_path = dir.join("pkg.toml");
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize pkg.toml: {}", e))?;
        fs::write(&manifest_path, content).map_err(|e| format!("failed to write pkg.toml: {}", e))
    }
}

/// Initialize a new mica project
pub fn init_project(dir: &Path, name: Option<&str>) -> Result<(), String> {
    // Determine project name
    let project_name = name
        .map(|s| s.to_string())
        .or_else(|| dir.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "myproject".to_string());

    // Check if pkg.toml already exists
    let manifest_path = dir.join("pkg.toml");
    if manifest_path.exists() {
        return Err(format!("pkg.toml already exists in {}", dir.display()));
    }

    // Create directory structure
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir).map_err(|e| format!("failed to create src directory: {}", e))?;

    // Create pkg.toml
    let manifest = PackageManifest::new(&project_name);
    manifest.save(dir)?;

    // Create src/main.mica with hello world
    let main_mica = src_dir.join("main.mica");
    if !main_mica.exists() {
        let content = r#"// Welcome to mica!
print("Hello, world!");
"#;
        fs::write(&main_mica, content).map_err(|e| format!("failed to write main.mica: {}", e))?;
    }

    println!(
        "Created new mica project '{}' in {}",
        project_name,
        dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn test_new_manifest() {
        let manifest = PackageManifest::new("testproject");
        assert_eq!(manifest.package.name, "testproject");
        assert_eq!(manifest.package.version, "0.1.0");
        assert_eq!(manifest.package.entry, "src/main.mica");
    }

    #[test]
    fn test_manifest_serialization() {
        let manifest = PackageManifest::new("testproject");
        let toml_str = toml::to_string_pretty(&manifest).unwrap();
        assert!(toml_str.contains("name = \"testproject\""));
        assert!(toml_str.contains("version = \"0.1.0\""));
    }

    #[test]
    fn test_init_project() {
        let temp = temp_dir().join("mica_test_init");
        if temp.exists() {
            fs::remove_dir_all(&temp).unwrap();
        }
        fs::create_dir_all(&temp).unwrap();

        init_project(&temp, Some("mytest")).unwrap();

        assert!(temp.join("pkg.toml").exists());
        assert!(temp.join("src/main.mica").exists());

        let manifest = PackageManifest::load(&temp).unwrap();
        assert_eq!(manifest.package.name, "mytest");

        fs::remove_dir_all(&temp).ok();
    }
}
