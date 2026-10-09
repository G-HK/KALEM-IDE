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

/// Suffix pairs `(dependent, parent)` for files that sit under another file with the same stem:
/// `site.css` under `site.scss`, `Form1.Designer.cs` under `Form1.cs`, ...
const PAIRS: &[(&str, &str)] = &[
    ("css", "scss"),
    ("min.css", "scss"),
    ("css", "less"),
    ("min.css", "less"),
    ("css.map", "scss"),
    ("js", "ts"),
    ("min.js", "ts"),
    ("js.map", "ts"),
    ("d.ts", "ts"),
    ("designer.cs", "cs"),
    ("designer.vb", "vb"),
    ("g.cs", "cs"),
    ("g.i.cs", "cs"),
    ("generated.cs", "cs"),
    ("designer.cs", "resx"),
    ("designer.cs", "settings"),
    ("designer.cs", "tt"),
    ("resx", "cs"),
];

/// Extensions whose `name.<variant>.ext` files sit under `name.ext` (`appsettings.Development.json`).
const VARIANTS: &[&str] = &["json", "config", "yml", "yaml"];

/// The index of the file each of `names` (files of one folder) nests under, like Visual Studio's
/// file nesting: `Gauge.razor.cs` and `Gauge.razor.css` under `Gauge.razor`, `Form1.Designer.cs`
/// under `Form1.cs`, `appsettings.Development.json` under `appsettings.json`, ...
pub fn nest_files(names: &[&str]) -> Vec<Option<usize>> {
    let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let find = |wanted: &str| lower.iter().position(|n| n == wanted);
    lower
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let parent = (|| {
                // Same stem, related extension.
                for (dep, par) in PAIRS {
                    if let Some(stem) = n.strip_suffix(&format!(".{dep}")) {
                        if let Some(p) = find(&format!("{stem}.{par}")) {
                            return Some(p);
                        }
                    }
                }
                // `A.B.C` under `A.B` (`.razor.cs`, `.xaml.cs`, `.cshtml.cs`, `.razor.css`, ...).
                if let Some((stem, _)) = n.rsplit_once('.') {
                    if !stem.is_empty() {
                        if let Some(p) = find(stem) {
                            return Some(p);
                        }
                    }
                }
                // `name.variant.json` under `name.json`.
                let (rest, ext) = n.rsplit_once('.')?;
                if VARIANTS.contains(&ext) {
                    let (base, _) = rest.rsplit_once('.')?;
                    return find(&format!("{base}.{ext}"));
                }
                None
            })();
            parent.filter(|&p| p != i)
        })
        .collect()
}

impl Node {
    /// This folder's tree with nested files grouped under their parent file (see [`nest_files`]);
    /// a file that has others under it carries them as `children`.
    pub fn nested(&self) -> Node {
        let files: Vec<usize> = (0..self.children.len())
            .filter(|&i| !self.children[i].is_dir)
            .collect();
        let names: Vec<&str> = files
            .iter()
            .map(|&i| self.children[i].name.as_str())
            .collect();
        let parents = nest_files(&names);
        // Guard against loops: a file whose chain of parents comes back to itself stays flat.
        let looped = |mut k: usize| {
            for _ in 0..=names.len() {
                match parents[k] {
                    Some(p) => k = p,
                    None => return false,
                }
            }
            true
        };
        let parent_of: Vec<Option<usize>> = (0..names.len())
            .map(|k| if looped(k) { None } else { parents[k] })
            .collect();
        fn build(node: &Node, files: &[usize], parent_of: &[Option<usize>], k: usize) -> Node {
            let mut n = node.children[files[k]].clone();
            n.children = (0..files.len())
                .filter(|&c| parent_of[c] == Some(k))
                .map(|c| build(node, files, parent_of, c))
                .collect();
            n
        }
        let mut children: Vec<Node> = Vec::new();
        for (i, c) in self.children.iter().enumerate() {
            if c.is_dir {
                children.push(c.nested());
            } else if let Some(k) = files.iter().position(|&f| f == i) {
                if parent_of[k].is_none() {
                    children.push(build(self, &files, &parent_of, k));
                }
            }
        }
        Node {
            children,
            ..self.clone()
        }
    }

    /// Whether this is a file with other files nested under it.
    pub fn is_nest_parent(&self) -> bool {
        !self.is_dir && !self.children.is_empty()
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

    #[test]
    fn razor_and_xaml_files_nest_under_their_component() {
        let n = nest_files(&[
            "Gauge.razor",
            "Gauge.razor.cs",
            "Gauge.razor.css",
            "Other.razor",
        ]);
        assert_eq!(n, vec![None, Some(0), Some(0), None]);
        let n = nest_files(&["Main.xaml", "Main.xaml.cs"]);
        assert_eq!(n, vec![None, Some(0)]);
    }

    #[test]
    fn designer_and_variant_files_nest() {
        let n = nest_files(&["Form1.cs", "Form1.Designer.cs", "Form1.resx"]);
        assert_eq!(n, vec![None, Some(0), Some(0)]);
        let n = nest_files(&[
            "appsettings.json",
            "appsettings.Development.json",
            "other.json",
        ]);
        assert_eq!(n, vec![None, Some(0), None]);
        let n = nest_files(&["site.scss", "site.css", "site.min.css", "site.css.map"]);
        assert_eq!(n, vec![None, Some(0), Some(0), Some(0)]);
    }

    #[test]
    fn unrelated_files_stay_flat() {
        let n = nest_files(&[
            "README.md",
            "main.rs",
            "lib.rs",
            "Cargo.toml",
            "x.cs",
            "y.cs",
        ]);
        assert!(n.iter().all(Option::is_none));
    }

    #[test]
    fn nested_tree_groups_children_and_recurses() {
        let e = Explorer::from_paths([
            "src/Pages/Gauge.razor",
            "src/Pages/Gauge.razor.cs",
            "src/Pages/Gauge.razor.css",
            "src/Pages/Plain.cs",
        ]);
        let t = e.root.nested();
        let pages = &t.children[0].children[0];
        let names: Vec<_> = pages.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Gauge.razor", "Plain.cs"]);
        let g = &pages.children[0];
        assert!(g.is_nest_parent());
        let kids: Vec<_> = g.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(kids, vec!["Gauge.razor.cs", "Gauge.razor.css"]);
        // the flat tree is untouched, so lookups keep working
        assert_eq!(
            e.find("src/Pages/Gauge.razor.cs").unwrap().name,
            "Gauge.razor.cs"
        );
    }
}
