use tuidom::Document;
use tuidom::style::{Color, Style};

/// The colors a theme declares, one per color variable components reference.
///
/// The names follow shadcn's theme variables. A `*_foreground` color is for text drawn on the
/// color it is paired with.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    /// `--background`: the root surface.
    pub background: Color,
    /// `--foreground`: default text.
    pub foreground: Color,
    /// `--card`: raised surfaces such as panels and modal bodies.
    pub card: Color,
    /// `--card-foreground`.
    pub card_foreground: Color,
    /// `--popover`: floating surfaces such as dropdowns, menus, tooltips, and toasts.
    pub popover: Color,
    /// `--popover-foreground`.
    pub popover_foreground: Color,
    /// `--primary`: the main action and selected state.
    pub primary: Color,
    /// `--primary-foreground`.
    pub primary_foreground: Color,
    /// `--secondary`: lower-emphasis actions and bars.
    pub secondary: Color,
    /// `--secondary-foreground`.
    pub secondary_foreground: Color,
    /// `--muted`: tracks and disabled fills.
    pub muted: Color,
    /// `--muted-foreground`: hints and secondary text.
    pub muted_foreground: Color,
    /// `--accent`: the hovered or highlighted row.
    pub accent: Color,
    /// `--accent-foreground`.
    pub accent_foreground: Color,
    /// `--destructive`: dangerous actions and errors.
    pub destructive: Color,
    /// `--success`. Not one of shadcn's variables.
    pub success: Color,
    /// `--warning`. Not one of shadcn's variables.
    pub warning: Color,
    /// `--border`: dividers and container borders.
    pub border: Color,
    /// `--input`: borders of input controls.
    pub input: Color,
    /// `--ring`: the focus indicator.
    pub ring: Color,
}

impl Palette {
    /// shadcn's dark neutral palette.
    pub fn dark() -> Self {
        Self {
            background: Color::oklch(0.145, 0.0, 0.0),
            foreground: Color::oklch(0.985, 0.0, 0.0),
            card: Color::oklch(0.205, 0.0, 0.0),
            card_foreground: Color::oklch(0.985, 0.0, 0.0),
            popover: Color::oklch(0.205, 0.0, 0.0),
            popover_foreground: Color::oklch(0.985, 0.0, 0.0),
            primary: Color::oklch(0.922, 0.0, 0.0),
            primary_foreground: Color::oklch(0.205, 0.0, 0.0),
            secondary: Color::oklch(0.269, 0.0, 0.0),
            secondary_foreground: Color::oklch(0.985, 0.0, 0.0),
            muted: Color::oklch(0.269, 0.0, 0.0),
            muted_foreground: Color::oklch(0.708, 0.0, 0.0),
            accent: Color::oklch(0.269, 0.0, 0.0),
            accent_foreground: Color::oklch(0.985, 0.0, 0.0),
            destructive: Color::oklch(0.704, 0.191, 22.216),
            success: Color::oklch(0.723, 0.219, 149.579),
            warning: Color::oklch(0.828, 0.189, 84.429),
            border: Color::oklcha(1.0, 0.0, 0.0, 0.1),
            input: Color::oklcha(1.0, 0.0, 0.0, 0.15),
            ring: Color::oklch(0.556, 0.0, 0.0),
        }
    }

    /// shadcn's light neutral palette, on an off-white background so white cards and popovers
    /// stand apart from it without a border. Secondary, muted and accent are darker than
    /// shadcn's, which are too close to that background to see.
    pub fn light() -> Self {
        Self {
            background: Color::oklch(0.96, 0.0, 0.0),
            foreground: Color::oklch(0.145, 0.0, 0.0),
            card: Color::oklch(1.0, 0.0, 0.0),
            card_foreground: Color::oklch(0.145, 0.0, 0.0),
            popover: Color::oklch(1.0, 0.0, 0.0),
            popover_foreground: Color::oklch(0.145, 0.0, 0.0),
            primary: Color::oklch(0.205, 0.0, 0.0),
            primary_foreground: Color::oklch(0.985, 0.0, 0.0),
            secondary: Color::oklch(0.92, 0.0, 0.0),
            secondary_foreground: Color::oklch(0.205, 0.0, 0.0),
            muted: Color::oklch(0.92, 0.0, 0.0),
            muted_foreground: Color::oklch(0.556, 0.0, 0.0),
            accent: Color::oklch(0.92, 0.0, 0.0),
            accent_foreground: Color::oklch(0.205, 0.0, 0.0),
            destructive: Color::oklch(0.577, 0.245, 27.325),
            success: Color::oklch(0.627, 0.194, 149.214),
            warning: Color::oklch(0.769, 0.188, 70.08),
            border: Color::oklch(0.922, 0.0, 0.0),
            input: Color::oklch(0.922, 0.0, 0.0),
            ring: Color::oklch(0.708, 0.0, 0.0),
        }
    }

    /// Each variable name with its color.
    pub fn vars(&self) -> [(&'static str, &Color); 20] {
        [
            ("--background", &self.background),
            ("--foreground", &self.foreground),
            ("--card", &self.card),
            ("--card-foreground", &self.card_foreground),
            ("--popover", &self.popover),
            ("--popover-foreground", &self.popover_foreground),
            ("--primary", &self.primary),
            ("--primary-foreground", &self.primary_foreground),
            ("--secondary", &self.secondary),
            ("--secondary-foreground", &self.secondary_foreground),
            ("--muted", &self.muted),
            ("--muted-foreground", &self.muted_foreground),
            ("--accent", &self.accent),
            ("--accent-foreground", &self.accent_foreground),
            ("--destructive", &self.destructive),
            ("--success", &self.success),
            ("--warning", &self.warning),
            ("--border", &self.border),
            ("--input", &self.input),
            ("--ring", &self.ring),
        ]
    }

    pub(crate) fn declare_on_document(&self, doc: &Document) {
        for (name, color) in self.vars() {
            doc.set_color_var(name, color.clone());
        }
    }

    pub(crate) fn declare_on(&self, style: &mut Style) {
        for (name, color) in self.vars() {
            style.color_var(name, color.clone());
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::dark()
    }
}
