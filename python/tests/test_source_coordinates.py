"""File APIs share source bytes; graph queries validate their coordinate space."""

import pytest

import cindergraph as cg


@pytest.mark.parametrize("newline", [b"\n", b"\r\n", b"\r"])
@pytest.mark.parametrize("heading", [b"// ascii", "// café".encode(), b"// bad\xff"])
def test_export_file_and_analysis_use_identical_decoded_source(
    tmp_path, newline, heading
):
    path = tmp_path / "input.c"
    raw = newline.join([heading, b"int f(void){", b"return 1;", b"}"])
    path.write_bytes(raw)
    report = cg.analyze_path(path)
    assert report.source == raw.decode("utf-8", errors="replace")
    for representation in cg.EXPORT_REPRS:
        assert cg.export_path(
            path, repr=representation, format="json"
        ) == cg.export_graphs(report.source, repr=representation, format="json")


def test_slice_checks_selected_function_node_count():
    code = "int f(void){return 1;} int g(int x){x++;x++;x++;return x;}"
    graphs = {g["name"]: g["cfg"] for g in cg.control_flow_graphs(code)}
    invalid = len(graphs["f"]["nodes"])
    assert invalid < len(graphs["g"]["nodes"])
    with pytest.raises(IndexError, match="outside function"):
        cg.backward_slice(code, "f", invalid)
    assert invalid in cg.backward_slice(code, "g", invalid)


def test_slice_unknown_function_keeps_key_error():
    with pytest.raises(KeyError, match="no function"):
        cg.backward_slice("int f(void){return 1;}", "missing", 0)
