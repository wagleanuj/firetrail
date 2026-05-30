//! `firetrail doc add` consumes the spec §5 frontmatter (firetrail-5lfs):
//! a `links:` entry auto-creates a `documented-in` edge and `doc_type`
//! overrides the `--type` flag — end-to-end through the real binary.

mod common;

use common::{fresh_repo, id_from_create, parse_json, run_firetrail};

#[test]
fn doc_add_frontmatter_links_create_documented_in_edge_and_override_type() {
    let tr = fresh_repo();
    let task = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "documented task"],
    ));

    let body = format!("---\ndoc_type: adr\nlinks:\n  - {task}\n---\n# Notes\n\nProse.\n");
    std::fs::write(tr.root().join("notes.md"), body).unwrap();

    let add = run_firetrail(
        tr.root(),
        &["--json", "doc", "add", "notes.md", "--type", "design"],
    );
    assert!(add.success(), "doc add failed: {}", add.stderr);
    let doc_id = parse_json(&add)["data"]["ids"][0]
        .as_str()
        .expect("doc add returns the new id")
        .to_string();

    // The frontmatter `links:` produced a documented-in edge — no `doc link`.
    let shown = run_firetrail(tr.root(), &["--json", "show", &task]);
    let v = parse_json(&shown);
    let relations = v["data"]["relations"].as_array().expect("relations array");
    assert!(
        relations.iter().any(|r| r["kind"] == "documented-in"),
        "expected a documented-in edge from frontmatter links: {relations:?}"
    );

    // Frontmatter `doc_type: adr` wins over `--type design`.
    let doc = run_firetrail(tr.root(), &["--json", "show", &doc_id]);
    let dv = parse_json(&doc);
    assert_eq!(dv["data"]["record"]["body"]["doc_type"], "adr");
}
