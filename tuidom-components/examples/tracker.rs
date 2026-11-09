//! A small task tracker built from `tuidom-components`, under the default theme. The tasks are
//! generated at start and kept in memory.
//!
//! Tab / Shift-Tab   — move focus (hovering focuses too)
//! Up / Down, PageUp / PageDown, Home / End — move through the projects or the tasks
//! Left / Right      — collapse or expand a project group; switch tabs while the tab strip is focused
//! Enter             — open the highlighted task in Details
//! Mouse drag        — move the divider between the projects and the tasks
//! n                 — new task (outside a text field)
//! ?                 — the key list (outside a text field)
//! q                 — quit (outside a text field)

use std::collections::HashSet;
use std::iter;
use tuidom::NodeKindView;
use tuidom::event::KeyCode;
use tuidom::style::{AlignItems, EdgeInsets, FlexDirection, FlexGap, Length, Overflow, Style};
use tuidom_components::{
    Button, ButtonProps, ButtonVariant, Checkbox, CheckboxProps, Column, KeyHint, KeyHintProps,
    Label, LabelProps, LabelTone, Menu, MenuItem, MenuProps, Modal, ModalProps, Panel, PanelProps,
    ProgressBar, ProgressBarProps, RadioGroup, RadioGroupProps, Select, SelectProps, Side, Split,
    SplitProps, StatusBar, StatusBarProps, Table, TableProps, Tabs, TabsProps, TextInput,
    TextInputProps, Theme, Toast, Toaster, ToasterHandle, ToasterProps, Tone, Tooltip,
    TooltipProps, Tree, TreeNode, TreeProps, install_theme, use_toaster,
};
use tuidom_ui::{
    App, Derived, Signal, View, component, derived, effect, fragment, state, use_document, view,
};

const GROUPS: [(&str, &[&str]); 3] = [
    ("Engineering", &["Engine", "Framework", "Components"]),
    ("Design", &["Website", "Docs"]),
    ("Operations", &["Infrastructure", "Release"]),
];
const STATUSES: [&str; 4] = ["Todo", "In progress", "Review", "Done"];
const DONE: usize = 3;
const PRIORITIES: [&str; 3] = ["Low", "Medium", "High"];
const PEOPLE: [&str; 6] = ["Ana", "Bram", "Chen", "Dana", "Emil", "Fatima"];
const VERBS: [&str; 8] = [
    "Fix", "Add", "Refactor", "Document", "Test", "Review", "Measure", "Remove",
];
const SUBJECTS: [&str; 12] = [
    "the layout cache",
    "scroll chaining",
    "the theme switch",
    "focus rings",
    "toast stacking",
    "table headers",
    "wide glyphs",
    "the release notes",
    "keyboard paging",
    "the landing page",
    "border charsets",
    "the build cache",
];
const TASKS: u32 = 2000;

#[derive(Debug, Clone, PartialEq)]
struct Task {
    id: u32,
    /// A project's id, `group/project`.
    project: String,
    title: String,
    status: usize,
    priority: usize,
    assignee: usize,
    blocked: bool,
}

fn slug(name: &str) -> String {
    name.to_lowercase()
}

fn project_id(group: &str, project: &str) -> String {
    format!("{}/{}", slug(group), slug(project))
}

fn project_ids() -> Vec<String> {
    GROUPS
        .iter()
        .flat_map(|(group, projects)| projects.iter().map(|p| project_id(group, p)))
        .collect()
}

/// The display name of a group or project id.
fn name_of(id: &str) -> String {
    GROUPS
        .iter()
        .flat_map(|(group, projects)| {
            iter::once((slug(group), group.to_string())).chain(
                projects
                    .iter()
                    .map(move |p| (project_id(group, p), format!("{group} › {p}"))),
            )
        })
        .find(|(candidate, _)| candidate == id)
        .map_or_else(|| "All projects".to_string(), |(_, name)| name)
}

fn in_scope(task: &Task, scope: Option<&str>) -> bool {
    match scope {
        None => true,
        Some(scope) => {
            task.project == scope
                || task
                    .project
                    .strip_prefix(scope)
                    .is_some_and(|rest| rest.starts_with('/'))
        }
    }
}

fn seed() -> Vec<Task> {
    let projects = project_ids();
    (0..TASKS)
        .map(|id| {
            let n = id as usize;
            Task {
                id: id + 1,
                project: projects[(n * 7 + n / 5) % projects.len()].clone(),
                title: format!(
                    "{} {}",
                    VERBS[(n * 5) % VERBS.len()],
                    SUBJECTS[(n * 7 + n / 3) % SUBJECTS.len()]
                ),
                status: (n * 3 + n / 11) % STATUSES.len(),
                priority: (n + n / 4) % PRIORITIES.len(),
                assignee: (n * 11) % PEOPLE.len(),
                blocked: n.is_multiple_of(17),
            }
        })
        .collect()
}

fn project_tree() -> Vec<TreeNode> {
    GROUPS
        .iter()
        .map(|(group, projects)| {
            TreeNode::new(slug(group), *group).with_children(
                projects
                    .iter()
                    .map(|p| TreeNode::new(project_id(group, p), *p))
                    .collect(),
            )
        })
        .collect()
}

fn options(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

fn task_columns() -> Vec<Column> {
    vec![
        Column::new("ID", 6),
        Column::new("Title", 34),
        Column::new("Status", 11),
        Column::new("Priority", 8),
        Column::new("Assignee", 8),
    ]
}

/// The fields of a task's form, bound to the inputs.
#[derive(Clone, Copy)]
struct Form {
    title: Signal<String>,
    status: Signal<usize>,
    priority: Signal<usize>,
    assignee: Signal<usize>,
    blocked: Signal<bool>,
}

impl Form {
    fn new() -> Self {
        Form {
            title: state(String::new()),
            status: state(0),
            priority: state(1),
            assignee: state(0),
            blocked: state(false),
        }
    }

    fn load(&self, task: &Task) {
        self.title.set(task.title.clone());
        self.status.set(task.status);
        self.priority.set(task.priority);
        self.assignee.set(task.assignee);
        self.blocked.set(task.blocked);
    }

    fn clear(&self) {
        self.title.set(String::new());
        self.status.set(0);
        self.priority.set(1);
        self.assignee.set(0);
        self.blocked.set(false);
    }

    fn write(&self, task: &mut Task) {
        task.title = self.title.get().trim().to_string();
        task.status = self.status.get();
        task.priority = self.priority.get();
        task.assignee = self.assignee.get();
        task.blocked = self.blocked.get();
    }
}

/// Everything the screen reads and writes, `Copy` so every handler can take it along.
#[derive(Clone, Copy)]
struct Tracker {
    tasks: Signal<Vec<Task>>,
    next_id: Signal<u32>,
    expanded: Signal<HashSet<String>>,
    /// The tree's selection: a group or a project.
    scope: Signal<Option<String>>,
    /// Indices into `tasks` of the ones in scope, in table order.
    visible: Derived<Vec<usize>>,
    /// Tasks in scope that are done, and all of them.
    progress: Derived<(usize, usize)>,
    row: Signal<usize>,
    tab: Signal<usize>,
    editing: Signal<Option<u32>>,
    details: Form,
    draft: Form,
    creating: Signal<bool>,
    help: Signal<bool>,
    ratio: Signal<f64>,
    toaster: Option<ToasterHandle>,
}

impl Tracker {
    fn new(toaster: Option<ToasterHandle>) -> Self {
        let tasks = state(seed());
        let scope = state(Some(slug(GROUPS[0].0)));
        let visible: Derived<Vec<usize>> = derived(move || {
            let scope = scope.get();
            tasks.with(|tasks| {
                tasks
                    .iter()
                    .enumerate()
                    .filter(|(_, task)| in_scope(task, scope.as_deref()))
                    .map(|(i, _)| i)
                    .collect()
            })
        });
        let progress = derived(move || {
            let visible = visible.get();
            let done = tasks.with(|tasks| {
                visible
                    .iter()
                    .filter(|&&i| tasks.get(i).is_some_and(|t| t.status == DONE))
                    .count()
            });
            (done, visible.len())
        });
        Tracker {
            tasks,
            next_id: state(TASKS + 1),
            expanded: state(HashSet::from([slug(GROUPS[0].0)])),
            scope,
            visible,
            progress,
            row: state(0),
            tab: state(0),
            editing: state(None),
            details: Form::new(),
            draft: Form::new(),
            creating: state(false),
            help: state(false),
            ratio: state(0.22),
            toaster,
        }
    }

    fn task_at(&self, row: usize) -> Option<Task> {
        let index = self.visible.try_with(|v| v.get(row).copied()).flatten()?;
        self.tasks.with(|tasks| tasks.get(index).cloned())
    }

    fn cell(&self, row: usize, column: usize) -> String {
        let Some(task) = self.task_at(row) else {
            return String::new();
        };
        match column {
            0 => format!("T-{}", task.id),
            1 if task.blocked => format!("{} (blocked)", task.title),
            1 => task.title,
            2 => STATUSES[task.status].to_string(),
            3 => PRIORITIES[task.priority].to_string(),
            _ => PEOPLE[task.assignee].to_string(),
        }
    }

    fn notify(&self, title: String, tone: Tone) {
        if let Some(toaster) = self.toaster {
            toaster.show(Toast {
                title,
                description: String::new(),
                tone,
            });
        }
    }

    fn open(&self, row: usize) {
        if let Some(task) = self.task_at(row) {
            self.details.load(&task);
            self.editing.set(Some(task.id));
            self.tab.set(1);
        }
    }

    fn save(&self) {
        let Some(id) = self.editing.get() else {
            return;
        };
        if self.details.title.get().trim().is_empty() {
            self.notify("A task needs a title".to_string(), Tone::Warning);
            return;
        }
        let details = self.details;
        self.tasks.update(|tasks| {
            if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                details.write(task);
            }
        });
        self.notify(format!("Saved T-{id}"), Tone::Success);
    }

    fn delete(&self) {
        let Some(id) = self.editing.get() else {
            return;
        };
        self.tasks.update(|tasks| tasks.retain(|t| t.id != id));
        self.editing.set(None);
        self.tab.set(0);
        self.notify(format!("Deleted T-{id}"), Tone::Destructive);
    }

    fn start_new(&self) {
        self.draft.clear();
        self.creating.set(true);
    }

    /// A new task lands in the selected project, or a group's first project.
    fn create(&self) {
        let scope = self.scope.get().unwrap_or_default();
        let projects = project_ids();
        let Some(project) = projects
            .iter()
            .find(|p| **p == scope || p.starts_with(&format!("{scope}/")))
            .or(projects.first())
            .cloned()
        else {
            return;
        };
        let id = self.next_id.get();
        let mut task = Task {
            id,
            project,
            title: String::new(),
            status: 0,
            priority: 0,
            assignee: 0,
            blocked: false,
        };
        self.draft.write(&mut task);
        self.tasks.update(|tasks| tasks.push(task));
        self.next_id.set(id + 1);
        self.creating.set(false);
        self.notify(format!("Created T-{id}"), Tone::Success);
    }
}

fn screen() -> Style {
    let mut style = Style::new();
    style.width(Length::Percent(100.0));
    style.height(Length::Percent(100.0));
    style.flex_direction(FlexDirection::Column);
    style
}

fn menu_bar() -> Style {
    let mut style = Style::new();
    style.flex_shrink(0.0);
    style.gap(FlexGap::new(0, 1));
    style
}

fn body() -> Style {
    let mut style = Style::new();
    style.flex_grow(1.0);
    style.flex_basis(Length::Cells(0));
    style.overflow(Overflow::Clip);
    style
}

fn pane() -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.flex_grow(1.0);
    style.padding(EdgeInsets::new(1, 1, 0, 1));
    style.gap(FlexGap::new(1, 0));
    style
}

fn column() -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style
}

fn row() -> Style {
    let mut style = Style::new();
    style.gap(FlexGap::new(0, 2));
    style.align_items(AlignItems::FlexStart);
    style
}

fn sized(width: u16) -> Style {
    let mut style = Style::new();
    style.width(Length::Cells(width));
    style
}

fn grow() -> Style {
    let mut style = Style::new();
    style.flex_grow(1.0);
    style
}

fn file_items() -> Vec<MenuItem> {
    vec![
        MenuItem::new("New task").hint("n"),
        MenuItem::new("Quit").hint("q").separated(),
    ]
}

fn task_items(editing: bool) -> Vec<MenuItem> {
    let item = |label: &str| {
        let item = MenuItem::new(label);
        if editing { item } else { item.disabled() }
    };
    vec![item("Save"), item("Delete")]
}

fn help_items() -> Vec<MenuItem> {
    vec![MenuItem::new("Keys").hint("?")]
}

fn form(form: Form) -> View {
    view! {
        box {
            style: column(),
            Label { text: "Title".to_string(), tone: LabelTone::Muted },
            TextInput {
                value: form.title,
                placeholder: "What needs doing".to_string(),
                style: sized(44),
            },
            box {
                style: row(),
                box {
                    style: column(),
                    Label { text: "Status".to_string(), tone: LabelTone::Muted },
                    Select { options: options(&STATUSES), selected: form.status },
                    Label { text: "Assignee".to_string(), tone: LabelTone::Muted },
                    Select { options: options(&PEOPLE), selected: form.assignee },
                },
                box {
                    style: column(),
                    Label { text: "Priority".to_string(), tone: LabelTone::Muted },
                    RadioGroup { options: options(&PRIORITIES), selected: form.priority },
                },
            },
            Checkbox { checked: form.blocked, label: "Blocked".to_string() },
        }
    }
}

fn tasks_pane(app: Tracker) -> View {
    view! {
        box {
            style: pane(),
            Label {
                text: format!(
                    "{} · {} tasks, {} done",
                    name_of(&app.scope.get().unwrap_or_default()),
                    app.progress.get().1,
                    app.progress.get().0,
                ),
                tone: LabelTone::Muted,
            },
            Table {
                columns: task_columns(),
                count: app.visible.get().len(),
                cell: move |(row, column): (usize, usize)| app.cell(row, column),
                selected: app.row,
                on_activate: move |row: usize| app.open(row),
                style: grow(),
            },
        }
    }
}

fn details_pane(app: Tracker) -> View {
    view! {
        box {
            style: pane(),
            match app.editing.get() {
                None => Label {
                    text: "Press Enter on a task in Tasks to edit it here.".to_string(),
                    tone: LabelTone::Muted,
                },
                Some(id) => Panel {
                    title: format!("T-{id}"),
                    { form(app.details) },
                    box {
                        style: row(),
                        Tooltip {
                            text: "Keep these changes".to_string(),
                            side: Side::Below,
                            Button { label: "Save".to_string(), on_press: move |_| app.save() },
                        },
                        Tooltip {
                            text: "Remove the task for good".to_string(),
                            side: Side::Below,
                            Button {
                                label: "Delete".to_string(),
                                variant: ButtonVariant::Destructive,
                                on_press: move |_| app.delete(),
                            },
                        },
                    },
                },
            }
        }
    }
}

fn new_task_modal(app: Tracker) -> View {
    view! {
        Modal {
            open: app.creating,
            title: "New task".to_string(),
            { form(app.draft) },
            box {
                style: row(),
                Button {
                    label: "Create".to_string(),
                    disabled: app.draft.title.get().trim().is_empty(),
                    on_press: move |_| app.create(),
                },
                Button {
                    label: "Cancel".to_string(),
                    variant: ButtonVariant::Secondary,
                    on_press: move |_| app.creating.set(false),
                },
            },
        }
    }
}

fn help_modal(app: Tracker) -> View {
    let lines = [
        "Tab / Shift-Tab   move focus",
        "Up / Down         move through projects or tasks",
        "Left / Right      collapse or expand a group, or switch tabs",
        "Enter             open the highlighted task",
        "n                 new task",
        "q                 quit",
    ];
    view! {
        Modal {
            open: app.help,
            title: "Keys".to_string(),
            each _, line in lines {
                Label { text: line.to_string() }
            },
            Button { label: "Close".to_string(), on_press: move |_| app.help.set(false) },
        }
    }
}

fn status_bar(app: Tracker) -> View {
    let share = derived(move || match app.progress.get() {
        (_, 0) => 0.0,
        (done, total) => done as f64 / total as f64,
    });
    view! {
        StatusBar {
            left: fragment([
                view! { KeyHint { key: "tab".to_string(), label: "focus".to_string() } },
                view! { KeyHint { key: "enter".to_string(), label: "open".to_string() } },
                view! { KeyHint { key: "n".to_string(), label: "new".to_string() } },
                view! { KeyHint { key: "?".to_string(), label: "keys".to_string() } },
                view! { KeyHint { key: "q".to_string(), label: "quit".to_string() } },
            ]),
            right: fragment([
                view! { ProgressBar { value: share.get(), tone: Tone::Success, style: sized(20) } },
                view! { text(format!("{}/{} done", app.progress.get().0, app.progress.get().1)) },
            ]),
        }
    }
}

#[component]
fn Screen() -> View {
    let app = Tracker::new(use_toaster());

    effect(move || {
        app.scope.get();
        app.row.set(0);
    });

    let doc = use_document().expect("a component body has a document");
    let keys = doc.clone();
    doc.on_key_press(doc.root(), move |key| {
        // Keys typed into a text field bubble here too; they are text, not shortcuts.
        let typing = keys
            .get_node(key.target())
            .is_some_and(|node| matches!(node.kind, NodeKindView::Input { .. }));
        if typing || app.creating.get() || app.help.get() {
            return;
        }
        match key.code {
            KeyCode::Char('n') => app.start_new(),
            KeyCode::Char('?') => app.help.set(true),
            KeyCode::Char('q') => keys.quit(),
            _ => {}
        }
    })
    .expect("registering the key handler");
    let quit = doc.clone();

    view! {
        box {
            style: screen(),
            box {
                style: menu_bar(),
                Menu {
                    label: "File".to_string(),
                    items: file_items(),
                    on_select: move |i: usize| match i {
                        0 => app.start_new(),
                        _ => quit.quit(),
                    },
                },
                Menu {
                    label: "Task".to_string(),
                    items: task_items(app.editing.get().is_some()),
                    on_select: move |i: usize| match i {
                        0 => app.save(),
                        _ => app.delete(),
                    },
                },
                Menu {
                    label: "Help".to_string(),
                    items: help_items(),
                    on_select: move |_: usize| app.help.set(true),
                },
            },
            box {
                style: body(),
                Split {
                    ratio: app.ratio,
                    first: view! {
                        Panel {
                            title: "Projects".to_string(),
                            style: grow(),
                            Tree {
                                nodes: project_tree(),
                                expanded: app.expanded,
                                selected: app.scope,
                                on_activate: move |_: String| app.tab.set(0),
                                style: grow(),
                            },
                        }
                    },
                    second: view! {
                        Tabs {
                            tabs: options(&["Tasks", "Details"]),
                            selected: app.tab,
                            style: grow(),
                            match app.tab.get() {
                                0 => { tasks_pane(app) },
                                _ => { details_pane(app) },
                            }
                        }
                    },
                },
            },
            { status_bar(app) },
            { new_task_modal(app) },
            { help_modal(app) },
        }
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    App::new(|| {
        install_theme(Theme::default());
        view! {
            Toaster {
                Screen {}
            }
        }
    })
    .run()
    .await
}
