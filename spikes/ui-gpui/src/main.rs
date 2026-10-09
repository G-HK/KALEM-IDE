//! GPUI + gpui-component spike: same screen as the egui spike.
use gpui_kit::component::{
    input::{Editor, EditorState},
    list::ListItem,
    tree::{tree, TreeItem, TreeState},
    Root,
};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use kalem_core::explorer::Node;
use kalem_spike_data::{demo, Demo};

struct Kalem {
    demo: Demo,
    tree: Entity<TreeState>,
    editor: Entity<EditorState>,
    active: usize,
}

fn to_item(n: &Node) -> TreeItem {
    let mut it = TreeItem::new(n.path.clone(), n.name.clone());
    if n.is_dir {
        it = it.children(n.children.iter().map(to_item));
    }
    it
}

impl Kalem {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let demo = demo();
        let items: Vec<TreeItem> = demo.explorer.root.children.iter().map(to_item).collect();
        let tree = cx.new(|cx| TreeState::new(cx).items(items));
        let text = demo.files[0].text;
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("rust")
                .line_number(true)
                .default_value(text)
        });
        let mut me = Self { demo, tree, editor, active: 0 };
        me.sync(cx);
        me
    }

    /// "Sync with active document": expand ancestors and select the file.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let path = self.demo.files[self.active].path;
        if let Some(plan) = self.demo.explorer.reveal(path) {
            let expand: std::collections::HashSet<String> = plan.expand.into_iter().collect();
            fn open(it: TreeItem, ex: &std::collections::HashSet<String>) -> TreeItem {
                let expanded = ex.contains(it.id.as_ref());
                let kids: Vec<TreeItem> = it.children.iter().cloned().map(|c| open(c, ex)).collect();
                TreeItem::new(it.id.clone(), it.label.clone()).children(kids).expanded(expanded)
            }
            let items: Vec<TreeItem> = self
                .demo
                .explorer
                .root
                .children
                .iter()
                .map(|n| open(to_item(n), &expand))
                .collect();
            let sel: SharedString = plan.select.into();
            self.tree.update(cx, |t, cx| {
                t.set_items(items, cx);
                if let Some(ix) = t.index_of(&sel) {
                    t.set_selected_index(Some(ix), cx);
                    t.scroll_to_item(ix, ScrollStrategy::Center);
                }
            });
        }
    }
}

fn header(t: &str) -> Div {
    div().px_2().py_1().text_xs().font_weight(FontWeight::BOLD).child(t.to_string())
}

impl Render for Kalem {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let st = &self.demo.git_status;
        let g = &self.demo.git;
        let tabs = div().flex().gap_2().px_2().py_1().children(
            self.demo.files.iter().enumerate().map(|(i, f)| {
                let name = f.path.rsplit('/').next().unwrap_or(f.path).to_string();
                div()
                    .id(("tab", i))
                    .px_2()
                    .cursor_pointer()
                    .when(i == self.active, |d| d.font_weight(FontWeight::BOLD))
                    .child(name)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.active = i;
                        let text = this.demo.files[i].text;
                        this.editor.update(cx, |e, cx| e.set_value(text, window, cx));
                        this.sync(cx);
                        cx.notify();
                    }))
            }),
        );
        let explorer = div()
            .w(px(280.))
            .h_full()
            .flex()
            .flex_col()
            .child(header("SOLUTION EXPLORER"))
            .child(div().px_2().text_xs().child(format!(
                "{} - {} projects",
                self.demo.solution_name, self.demo.project_count
            )))
            .child(
                tree(&self.tree, |ix, entry, _sel, _w, _cx| {
                    ListItem::new(ix)
                        .pl(px(8. + 14. * entry.depth() as f32))
                        .child(entry.item().label.clone())
                })
                .flex_1(),
            );
        let git = div()
            .flex_1()
            .flex()
            .flex_col()
            .child(header("GIT CHANGES"))
            .child(div().px_2().text_xs().child(format!(
                "{} -> {} | ahead {} behind {}",
                g.branch.clone().unwrap_or_default(),
                g.upstream.clone().unwrap_or_default(),
                g.ahead,
                g.behind
            )))
            .children(st.entries.iter().map(|e| {
                div().px_2().text_xs().child(format!(
                    "{}{} {}",
                    if e.staged.is_some() { "S " } else { "  " },
                    if e.unstaged.is_some() { "M" } else { " " },
                    e.path
                ))
            }));
        let tests = div()
            .flex_1()
            .flex()
            .flex_col()
            .child(header("TEST RESULTS"))
            .children(self.demo.tests.cases.iter().take(12).map(|c| {
                div()
                    .px_2()
                    .text_xs()
                    .child(format!("{:?}  {}::{}", c.outcome, c.suite, c.name))
            }));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .child(explorer)
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(tabs)
                            .child(Editor::new(&self.editor).h_full()),
                    ),
            )
            .child(div().h(px(200.)).flex().child(git).child(tests))
            .child(
                div()
                    .h(px(22.))
                    .px_2()
                    .text_xs()
                    .child(format!("main | {}", self.demo.files[self.active].path)),
            )
    }
}

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let view = cx.new(|cx| Kalem::new(window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .expect("window");
        cx.activate(true);
    });
}
