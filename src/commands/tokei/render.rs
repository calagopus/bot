use super::{LanguageStats, RepositoryStats, colors::language_color, format_number, format_share};
use std::{fmt::Write, sync::LazyLock};
use takumi::prelude::{
    FontResource, Fonts, FromHtml, FromHtmlOptions, Node, OutputFormat, RenderOptions, Viewport,
};

const WIDTH: u32 = 800;
const DEVICE_PIXEL_RATIO: u32 = 2;
const MAX_BAR_LANGUAGES: usize = 8;
const MIN_BAR_SHARE: f64 = 0.01;
const OTHER_COLOR: u32 = 0x6e7681;

static FONTS: LazyLock<Fonts> = LazyLock::new(|| {
    let mut fonts = Fonts::default();
    fonts
        .register(FontResource::new(
            &include_bytes!("../../../assets/fonts/InterVariable.ttf")[..],
        ))
        .expect("failed to register tokei font");
    fonts
});

pub struct LanguageImages {
    pub overview: Vec<u8>,
    pub table: Vec<u8>,
}

struct BarSegment<'a> {
    name: &'a str,
    code: usize,
    color: u32,
}

fn bar_segments(stats: &RepositoryStats) -> Vec<BarSegment<'_>> {
    let total = stats.total.code.max(1) as f64;

    let (shown, other): (Vec<_>, Vec<_>) =
        stats
            .languages
            .iter()
            .enumerate()
            .partition(|(i, language)| {
                *i < MAX_BAR_LANGUAGES && language.code as f64 / total >= MIN_BAR_SHARE
            });

    let mut segments = shown
        .into_iter()
        .map(|(_, language)| BarSegment {
            name: &language.name,
            code: language.code,
            color: language_color(&language.name),
        })
        .collect::<Vec<_>>();

    let other = other
        .iter()
        .map(|(_, language)| language.code)
        .sum::<usize>();
    if other > 0 {
        segments.push(BarSegment {
            name: "Other",
            code: other,
            color: OTHER_COLOR,
        });
    }

    segments
}

fn page(content: &str) -> String {
    format!(
        r#"<div style="display:flex;flex-direction:column;padding:28px 32px;background:#222222;color:#f2f3f5;font-family:Inter;font-size:16px;line-height:1.4">{content}</div>"#
    )
}

fn overview_html(stats: &RepositoryStats) -> String {
    let total = stats.total.code;

    let mut html = format!(
        r#"<div style="display:flex;flex-direction:column;gap:14px">
<div style="display:flex;flex-direction:column;gap:2px">
<div style="display:flex;align-items:baseline;gap:10px"><span style="font-size:34px;font-weight:800;color:#74c0fc;font-variant-numeric:tabular-nums">{}</span><span style="font-size:18px;color:#c9cdd3">lines of code</span></div>
<div style="font-size:14px;color:#9ca3af">in {} files, plus {} comments and {} blank lines</div>
</div>"#,
        format_number(total),
        format_number(stats.total.files),
        format_number(stats.total.comments),
        format_number(stats.total.blanks),
    );

    let segments = bar_segments(stats);
    html.push_str(
        r#"<div style="display:flex;height:14px;gap:2px;border-radius:7px;overflow:hidden">"#,
    );
    for segment in &segments {
        let _ = write!(
            html,
            r#"<div style="flex:{} 1 0;background:#{:06x}"></div>"#,
            segment.code, segment.color
        );
    }
    html.push_str("</div>");

    html.push_str(r#"<div style="display:flex;flex-wrap:wrap;gap:6px 18px;font-size:14px">"#);
    for segment in &segments {
        let _ = write!(
            html,
            r#"<div style="display:flex;align-items:center;gap:6px"><div style="width:10px;height:10px;border-radius:5px;background:#{:06x}"></div><span style="font-weight:600">{}</span><span style="color:#9ca3af">{}</span></div>"#,
            segment.color,
            segment.name,
            format_share(segment.code, total),
        );
    }
    html.push_str("</div></div>");

    page(&html)
}

fn table_row(cells: [String; 6], color: Option<u32>, bold: bool, border: bool) -> String {
    let [name, share, files, code, comments, blanks] = cells;

    let dot = match color {
        Some(color) => format!(
            r#"<div style="width:10px;height:10px;border-radius:5px;background:#{color:06x}"></div>"#
        ),
        None => r#"<div style="width:10px"></div>"#.to_string(),
    };

    let mut row = format!(
        r#"<div style="display:flex;align-items:center;padding:8px 0;{}{}">
<div style="display:flex;align-items:center;gap:10px;flex:1">{dot}<span>{name}</span></div>"#,
        if border {
            "border-bottom:1px solid #3a3a3a;"
        } else {
            ""
        },
        if bold { "font-weight:700;" } else { "" },
    );
    for cell in [share, files, code, comments, blanks] {
        let _ = write!(
            row,
            r#"<div style="width:96px;text-align:right">{cell}</div>"#
        );
    }
    row.push_str("</div>");

    row
}

fn language_cells(language: &LanguageStats, total: usize) -> [String; 6] {
    [
        language.name.clone(),
        format_share(language.code, total),
        format_number(language.files),
        format_number(language.code),
        format_number(language.comments),
        format_number(language.blanks),
    ]
}

fn table_html(stats: &RepositoryStats) -> String {
    let total = stats.total.code;

    let mut html = String::from(
        r#"<div style="display:flex;flex-direction:column;font-size:14px;font-variant-numeric:tabular-nums">"#,
    );
    html.push_str(&table_row(
        ["Language", "Share", "Files", "Code", "Comments", "Blanks"]
            .map(|cell| format!(r#"<span style="color:#9ca3af;font-weight:600">{cell}</span>"#)),
        None,
        false,
        true,
    ));
    for language in stats.languages.iter().take(super::MAX_LANGUAGES) {
        html.push_str(&table_row(
            language_cells(language, total),
            Some(language_color(&language.name)),
            false,
            true,
        ));
    }
    if stats.languages.len() > super::MAX_LANGUAGES {
        let other = &stats.languages[super::MAX_LANGUAGES..];
        let other = LanguageStats {
            name: format!("Other ({})", other.len()),
            files: other.iter().map(|l| l.files).sum(),
            code: other.iter().map(|l| l.code).sum(),
            comments: other.iter().map(|l| l.comments).sum(),
            blanks: other.iter().map(|l| l.blanks).sum(),
        };

        html.push_str(&table_row(
            language_cells(&other, total),
            Some(OTHER_COLOR),
            false,
            true,
        ));
    }
    html.push_str(&table_row(
        language_cells(&stats.total, total),
        None,
        true,
        false,
    ));
    html.push_str("</div>");

    page(&html)
}

fn render_png(html: &str) -> Result<Vec<u8>, anyhow::Error> {
    let node = Node::from_html(html, FromHtmlOptions::default())?;
    let image = takumi::render(
        RenderOptions::builder()
            .viewport(
                Viewport::new((Some(WIDTH * DEVICE_PIXEL_RATIO), None))
                    .with_device_pixel_ratio(DEVICE_PIXEL_RATIO as f32),
            )
            .node(node)
            .fonts(&FONTS)
            .build(),
    )?;

    let mut png = Vec::new();
    takumi::write_image(&image, &mut png, OutputFormat::Png)?;

    Ok(png)
}

pub async fn render_images(stats: &RepositoryStats) -> Result<LanguageImages, anyhow::Error> {
    let overview = overview_html(stats);
    let table = table_html(stats);

    tokio::task::spawn_blocking(move || {
        Ok(LanguageImages {
            overview: render_png(&overview)?,
            table: render_png(&table)?,
        })
    })
    .await?
}
