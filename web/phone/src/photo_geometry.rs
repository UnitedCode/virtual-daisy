//! Hit areas follow the photographed surfaces; the pixels remain photographs.
use web_sys::{Document, Element};
type Point = (f64, f64);
const KEYS: [[Point; 4]; 12] = [
    [(40.5, 34.5), (45.2, 35.9), (43.6, 40.2), (39.2, 38.9)],
    [(46.7, 36.3), (51.5, 37.7), (49.8, 42.3), (45.3, 40.8)],
    [(53.1, 38.1), (57.7, 39.4), (56.2, 44.1), (51.6, 42.7)],
    [(38.4, 40.6), (42.8, 41.9), (41.3, 46.6), (36.6, 45.3)],
    [(44.4, 42.3), (49.3, 43.8), (47.6, 48.5), (42.8, 47.1)],
    [(50.9, 44.1), (55.8, 45.6), (53.9, 50.4), (49.3, 48.9)],
    [(36.1, 46.7), (40.7, 48.1), (39., 53.), (34.3, 51.5)],
    [(42.3, 48.5), (47.3, 50.), (45.5, 55.1), (40.7, 53.5)],
    [(48.7, 50.4), (53.6, 52.), (51.8, 57.2), (46.9, 55.7)],
    [(33.6, 52.8), (38.4, 54.4), (36.5, 59.6), (31.7, 57.8)],
    [(40., 54.7), (44.9, 56.3), (43.1, 61.7), (38.1, 59.9)],
    [(46.5, 56.6), (51.5, 58.3), (49.6, 63.6), (44.5, 61.9)],
];
fn align(element: Element, points: &[Point]) {
    let left = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let top = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let width = points.iter().map(|p| p.0).fold(0., f64::max) - left;
    let height = points.iter().map(|p| p.1).fold(0., f64::max) - top;
    let polygon = points
        .iter()
        .map(|p| {
            format!(
                "{:.5}% {:.5}%",
                (p.0 - left) / width * 100.,
                (p.1 - top) / height * 100.
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    element.set_attribute("style",&format!("--left:{left}%;--top:{top}%;--width:{width}%;--height:{height}%;--image-left:-{left}cqw;--image-top:-{}cqw;--cut:polygon({polygon})",top*1205./1306.)).unwrap();
    element.class_list().add_1("aligned").unwrap();
}
pub fn align_controls(document: &Document) {
    for (index, points) in KEYS.iter().enumerate() {
        align(
            document
                .query_selector(&format!("[data-key='{index}']"))
                .unwrap()
                .unwrap(),
            points,
        );
    }
    align(
        document.get_element_by_id("encoder").unwrap(),
        &[(78.6, 41.8), (86.4, 43.9), (85., 49.3), (77.3, 47.2)],
    );
    align(
        document.get_element_by_id("handset").unwrap(),
        &[
            (26.5, 8.),
            (29., 2.5),
            (39.3, 1.),
            (56.7, 3.4),
            (75.2, 7.9),
            (93.1, 13.9),
            (98., 18.3),
            (99.5, 26.4),
            (98.3, 33.4),
            (95., 37.1),
            (87.7, 37.7),
            (80.4, 34.9),
            (72., 27.),
            (31.8, 24.6),
            (27., 22.3),
            (25.1, 18.3),
            (24.4, 13.4),
        ],
    );
}
