//! ASCII STL writer.

use std::fmt::Write;

use crate::mesh::Triangle;

/// ASCII STL of `triangles`, coordinates in millimetres (the usual STL convention).
pub fn ascii(name: &str, triangles: &[Triangle]) -> String {
    let mut out = String::with_capacity(triangles.len() * 200);
    let name: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    // Writing to a String cannot fail.
    let _ = writeln!(out, "solid {name}");
    for t in triangles {
        let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
        let _ = writeln!(out, "  facet normal {:e} {:e} {:e}", n.x, n.y, n.z);
        let _ = writeln!(out, "    outer loop");
        for p in t {
            let p = *p * 1e3;
            let _ = writeln!(out, "      vertex {:e} {:e} {:e}", p.x, p.y, p.z);
        }
        let _ = writeln!(out, "    endloop");
        let _ = writeln!(out, "  endfacet");
    }
    let _ = writeln!(out, "endsolid {name}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tp_math::DVec3;

    #[test]
    fn single_facet() {
        let t = [DVec3::ZERO, DVec3::X, DVec3::Y];
        let text = ascii("my part", &[t]);
        assert!(text.starts_with("solid mypart\n"));
        assert!(text.contains("facet normal 0e0 0e0 1e0"));
        assert!(text.contains("vertex 1e3 0e0 0e0"));
        assert_eq!(text.matches("vertex").count(), 3);
        assert!(text.trim_end().ends_with("endsolid mypart"));
    }
}
