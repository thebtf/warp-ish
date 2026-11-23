use std::collections::HashMap;

use markdown_parser::html_parser::WARP_EMBED_ATTRIBUTE_NAME;
use serde_yaml::Mapping;
use warp_editor::content::markdown::MarkdownStyle;
use warp_editor::render::layout::TextLayout;
use warp_editor::render::model::{
    BROKEN_LINK_SPACING, BrokenBlockEmbedding, EmbeddedItem, EmbeddedItemHTMLRepresentation,
    EmbeddedItemRichFormat, LaidOutEmbeddedItem,
};
use warpui::AppContext;

/// Opaque Warp Drive markup from an existing Markdown document.
#[derive(Debug)]
pub(super) struct LegacyWorkflowEmbed {
    hashed_id: String,
    mapping: Mapping,
}

impl LegacyWorkflowEmbed {
    pub(super) fn from_mapping(mapping: Mapping) -> Option<Self> {
        let hashed_id = mapping
            .get(&serde_yaml::Value::String("id".into()))?
            .as_str()?
            .to_string();
        Some(Self { hashed_id, mapping })
    }
}

impl EmbeddedItem for LegacyWorkflowEmbed {
    fn layout(&self, text_layout: &TextLayout, _app: &AppContext) -> Box<dyn LaidOutEmbeddedItem> {
        Box::new(BrokenBlockEmbedding::new(
            text_layout.max_width() - BROKEN_LINK_SPACING.x_axis_offset(),
            text_layout.rich_text_styles().base_text.font_size,
        ))
    }

    fn hashed_id(&self) -> &str {
        &self.hashed_id
    }

    fn to_mapping(&self, _style: MarkdownStyle) -> Mapping {
        self.mapping.clone()
    }

    fn to_rich_format(&self, _app: &AppContext) -> EmbeddedItemRichFormat<'_> {
        EmbeddedItemRichFormat {
            plain_text: String::new(),
            html: EmbeddedItemHTMLRepresentation {
                element_name: "pre",
                content: String::new(),
                attributes: HashMap::from([(WARP_EMBED_ATTRIBUTE_NAME, self.hashed_id())]),
            },
        }
    }
}
