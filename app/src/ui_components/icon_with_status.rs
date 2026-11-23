use warp_core::ui::icons::Icon as WarpIcon;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::theme::{Fill as WarpThemeFill, WarpTheme};
use warpui::elements::{ConstrainedBox, Container, CornerRadius, Element, ParentElement, Radius};

const NEUTRAL_GLYPH_RATIO: f32 = 16.0 / 24.0;

pub(crate) enum IconWithStatusVariant {
    /// A generic icon with a given color on an overlay background.
    Neutral {
        icon: WarpIcon,
        icon_color: WarpThemeFill,
    },
    /// A pre-built icon element on an overlay background.
    NeutralElement { icon_element: Box<dyn Element> },
}

pub(crate) fn render_icon_with_status(
    variant: IconWithStatusVariant,
    total_size: f32,
    theme: &WarpTheme,
) -> Box<dyn Element> {
    let icon_element = match variant {
        IconWithStatusVariant::Neutral { icon, icon_color } => {
            icon.to_warpui_icon(icon_color).finish()
        }
        IconWithStatusVariant::NeutralElement { icon_element } => icon_element,
    };
    render_neutral_circle(icon_element, internal_colors::fg_overlay_2(theme), total_size)
}

fn render_neutral_circle(
    icon_element: Box<dyn Element>,
    background: WarpThemeFill,
    total_size: f32,
) -> Box<dyn Element> {
    let glyph = total_size * NEUTRAL_GLYPH_RATIO;
    let padding = (total_size - glyph) / 2.;
    let inner = ConstrainedBox::new(icon_element)
        .with_width(glyph)
        .with_height(glyph)
        .finish();
    Container::new(inner)
        .with_uniform_padding(padding)
        .with_background(background)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(total_size / 2.)))
        .finish()
}
