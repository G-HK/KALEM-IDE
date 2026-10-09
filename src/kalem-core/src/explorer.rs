//! Project Explorer model: the tree behind the Solution/Project Explorer panel.
//!
//! The panel shows an overview of the project and its items. When the user switches to a file in
//! the editor, "sync with active document" expands the folders above it and selects it, using
//! [`Explorer::reveal`].

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    /// Path from the project root, `/` separated (empty for the root).
    pub path: String,
    pub is_dir: bool,
    pub children: Vec<Node>,
}

/// What the UI should do to show a file: open these folders, then select this item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevealPlan {
    pub expand: Vec<String>,
    pub select: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explorer {
    pub root: Node,
}

/// Counts for the overview line at the top of the panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TreeStats {
    pub folders: usize,
    pub files: usize,
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_matches('/').to_string()
}

fn order(a: &Node, b: &Node) -> Ordering {
    // Folders first, then files; case-insensitive alphabetical inside each group.
    b.is_dir
        .cmp(&a.is_dir)
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.name.cmp(&b.name))
}

fn sort_tree(node: &mut Node) {
    node.children.sort_by(order);
    for c in &mut node.children {
        sort_tree(c);
    }
}

impl Explorer {
    /// Build the tree from file paths relative to the project root (either slash style).
    pub fn from_paths<I, S>(paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut root = Node {
            name: String::new(),
            path: String::new(),
            is_dir: true,
            children: Vec::new(),
        };
        for p in paths {
            let p = normalize(p.as_ref());
            if p.is_empty() {
                continue;
            }
            let parts: Vec<&str> = p.split('/').collect();
            let mut cur = &mut root;
            let mut acc = String::new();
            for (i, part) in parts.iter().enumerate() {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(part);
                let is_dir = i + 1 < parts.len();
                let idx = match cur
                    .children
                    .iter()
                    .position(|c| c.name == *part && c.is_dir == is_dir)
                {
                    Some(idx) => idx,
                    None => {
                        cur.children.push(Node {
                            name: (*part).to_string(),
                            path: acc.clone(),
                            is_dir,
                            children: Vec::new(),
                        });
                        cur.children.len() - 1
                    }
                };
                cur = &mut cur.children[idx];
            }
        }
        sort_tree(&mut root);
        Explorer { root }
    }

    pub fn find(&self, path: &str) -> Option<&Node> {
        let path = normalize(path);
        let mut cur = &self.root;
        if path.is_empty() {
            return Some(cur);
        }
        for part in path.split('/') {
            cur = cur.children.iter().find(|c| c.name == part)?;
        }
        Some(cur)
    }

    /// Sync the tree with the active document. `None` if the file is not part of the project.
    pub fn reveal(&self, path: &str) -> Option<RevealPlan> {
        let path = normalize(path);
        let node = self.find(&path)?;
        if path.is_empty() {
            return None;
        }
        let parts: Vec<&str> = path.split('/').collect();
        let mut expand = Vec::new();
        let mut acc = String::new();
        // Every ancestor folder; a selected folder itself is not expanded.
        for part in &parts[..parts.len() - 1] {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(part);
            expand.push(acc.clone());
        }
        Some(RevealPlan {
            expand,
            select: node.path.clone(),
        })
    }

    pub fn stats(&self) -> TreeStats {
        fn walk(n: &Node, s: &mut TreeStats) {
            for c in &n.children {
                if c.is_dir {
                    s.folders += 1;
                } else {
                    s.files += 1;
                }
                walk(c, s);
            }
        }
        let mut s = TreeStats::default();
        walk(&self.root, &mut s);
        s
    }

    /// Flattened visible rows `(depth, node)` given the set of expanded folder paths.
    pub fn visible_rows<'a>(&'a self, expanded: &[String]) -> Vec<(usize, &'a Node)> {
        fn walk<'a>(
            n: &'a Node,
            depth: usize,
            expanded: &[String],
            out: &mut Vec<(usize, &'a Node)>,
        ) {
            for c in &n.children {
                out.push((depth, c));
                if c.is_dir && expanded.iter().any(|e| e == &c.path) {
                    walk(c, depth + 1, expanded, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, 0, expanded, &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Explorer {
        Explorer::from_paths([
            "src/Veto/_Imports.razor",
            "src\\Veto\\VetoServiceCollectionExtensions.cs",
            "src/Veto.Core/Core.cs",
            "src/Components/Button.razor",
            "docs/readme.md",
            "tests/a.cs",
            "README.md",
            ".github/ci.yml",
        ])
    }

    #[test]
    fn folders_first_then_files_sorted_case_insensitively() {
        let e = sample();
        let names: Vec<_> = e.root.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec![".github", "docs", "src", "tests", "README.md"]);
        let src: Vec<_> = e
            .find("src")
            .unwrap()
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(src, vec!["Components", "Veto", "Veto.Core"]);
    }

    #[test]
    fn backslash_paths_land_in_the_same_folder() {
        let e = sample();
        let veto: Vec<_> = e
            .find("src/Veto")
            .unwrap()
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(
            veto,
            vec!["_Imports.razor", "VetoServiceCollectionExtensions.cs"]
        );
    }

    #[test]
    fn reveal_expands_ancestors_and_selects_the_file() {
        let e = sample();
        let plan = e.reveal("src\\Veto\\_Imports.razor").unwrap();
        assert_eq!(plan.expand, vec!["src", "src/Veto"]);
        assert_eq!(plan.select, "src/Veto/_Imports.razor");
        assert_eq!(e.reveal("README.md").unwrap().expand, Vec::<String>::new());
        assert_eq!(e.reveal("nope/x.rs"), None);
    }

    #[test]
    fn visible_rows_follow_expansion() {
        let e = sample();
        assert_eq!(e.visible_rows(&[]).len(), 5);
        let plan = e.reveal("src/Veto/_Imports.razor").unwrap();
        let rows = e.visible_rows(&plan.expand);
        let selected = rows.iter().find(|(_, n)| n.path == plan.select).unwrap();
        assert_eq!(selected.0, 2); // src(0) / Veto(1) / file(2)
    }

    #[test]
    fn stats_count_folders_and_files() {
        let s = sample().stats();
        assert_eq!((s.folders, s.files), (7, 8));
    }
}
