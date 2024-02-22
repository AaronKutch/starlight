#![allow(clippy::type_complexity)]

use std::{
    fmt::Write,
    fs::{self, File, OpenOptions},
    io,
    path::PathBuf,
};

/// Minimal renderer for basic debugging and visualization
#[derive(Debug, Clone, Default)]
pub struct Render {
    /// Rectangles in (x position, y position, x width, y width)
    pub rects: Vec<(i32, i32, i32, i32, String)>,
    /// Text location, size, length, and text
    pub text: Vec<((i32, i32), i32, i32, String)>,
    /// Location of line endpoints, line width, and color
    pub lines: Vec<((i32, i32), (i32, i32), i32, String)>,
    /// Location, radius, color
    pub circles: Vec<((i32, i32), i32, String)>,
    /// Total dimensions
    pub total_dim: (i32, i32),
}

impl Render {
    // Quaternion VScode color theme
    // gray, blue, cyan, green, yellow, peach, magenta, purple
    pub const COLORS: [&'static str; 8] = [
        "a0a0a0", "00b2ff", "00cb9d", "2ab90f", "c49d00", "ff8080", "ff2adc", "a35bff",
    ];
    pub const EIGENGRAU: &'static str = "171717";
    pub const FONT_FAMILY: &'static str = "monospace";
    pub const PAD: i32 = 128;
    pub const TEXT_COLOR: &'static str = "a0a0a0";

    /// Creates a new `Render` struct with total dimensions `total_dim`
    pub fn new(total_dim: (i32, i32)) -> Self {
        Self {
            total_dim,
            ..Default::default()
        }
    }

    /// Creates and appends SVG to the file at `out_file`
    pub fn render_to_svg_file(&self, out_file: PathBuf) {
        let s = gen_svg(self);
        drop(fs::remove_file(&out_file));
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(out_file)
            .unwrap();
        // avoid trait collision
        <File as io::Write>::write_all(&mut file, s.as_bytes()).unwrap();
    }
}

/// create the SVG code
fn gen_svg(g: &Render) -> String {
    // example code for future reference
    //<circle cx="128" cy="128" r="128" fill="orange"/>
    //<rect fill="#f00" x="64" y="64" width="64" height="64"/>
    //<text x="64" y="64" font-size="16" font-family="monospace"
    //<text font-weight="normal">oO0iIlL</text>
    //<!-- L is for line Q is for quadratic C is for cubic -->>
    //<path d="M 0,0 C 100,0 100,100 0,100" stroke="#00f" fill="#000a"/>
    //<!-- grouping element <g> </g> that can apply common modifiers such as
    //<!-- `opacity="0.5"` -->
    //<!--
    //<rect fill="#fff" stroke="#000" x="0" y="0" width="300" height="300"/>
    //<rect x="25" y="25" width="500" height="200" fill="none" stroke-width="4"
    //<rect stroke="pink" />
    //<circle cx="125" cy="125" r="75" fill="orange" />
    //<polyline points="50,150 50,200 200,200 200,100" stroke="red" stroke-width="4"
    //<polyline fill="none" />
    //<line x1="50" y1="50" x2="200" y2="200" stroke="blue" stroke-width="4" />
    //-->

    let mut s = String::new();

    // for debug
    //s += &format!("<circle cx=\"{}\" cy=\"{}\" r=\"4\" fill=\"#f0f\"/>", tmp.0
    // .0, tmp.0 .1);

    // background
    write!(
        s,
        "<rect fill=\"#000\" x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"/>",
        g.total_dim.0, g.total_dim.1
    )
    .unwrap();

    // rectangles and outlines first
    for rect in &g.rects {
        writeln!(
            s,
            "<rect fill=\"#{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
            rect.4, rect.0, rect.1, rect.2, rect.3
        )
        .unwrap();
        // outline the rectangle
        writeln!(
            s,
            "<polyline stroke=\"#0000\" stroke-width=\"0\" points=\"{},{} {},{} {},{} {},{} \
             {},{}\"  fill=\"#0000\"/>",
            rect.0,
            rect.1,
            rect.0 + rect.2,
            rect.1,
            rect.0 + rect.2,
            rect.1 + rect.3,
            rect.0,
            rect.1 + rect.3,
            rect.0,
            rect.1,
        )
        .unwrap();
    }

    // lines second
    for line in &g.lines {
        writeln!(
            s,
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#{}\" stroke-width=\"{}\" />",
            line.0 .0, line.0 .1, line.1 .0, line.1 .1, line.3, line.2
        )
        .unwrap();
    }

    // circles third
    for circle in &g.circles {
        writeln!(
            s,
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#{}\" />",
            circle.0 .0, circle.0 .1, circle.1, circle.2
        )
        .unwrap();
    }

    // text last
    for tmp in &g.text {
        let size = tmp.1;
        // these characters need to be escaped, do "&" first
        let final_text = tmp
            .3
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        writeln!(
            s,
            "<text fill=\"#{}\" font-size=\"{}\" font-family=\"{}\" x=\"{}\" y=\"{}\" \
             textLength=\"{}\">{}</text>",
            Render::TEXT_COLOR,
            size,
            Render::FONT_FAMILY,
            tmp.0 .0,
            tmp.0 .1,
            tmp.2,
            final_text
        )
        .unwrap();
    }

    let output = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n\
        <!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \
        \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n\
        <svg preserveAspectRatio=\"meet\" viewBox=\"{} {} {} {}\" width=\"100%\" height=\"100%\" \
        version=\"1.1\" xmlns=\"http://www.w3.org/2000/svg\">\n\
        {}\n\
        </svg>",
        -Render::PAD,
        -Render::PAD,
        g.total_dim.0 + Render::PAD,
        g.total_dim.1 + Render::PAD,
        s,
    );

    output
}
