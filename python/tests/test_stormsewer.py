"""The Python surface, checked against VALIDATION.md.

Every expected value here is published in VALIDATION.md with its hand
calculation. If one moves, the document is now wrong: update both together.
"""

import pathlib

import pytest

import stormsewer as ss

REPO = pathlib.Path(__file__).resolve().parents[2]
SAMPLE = REPO / "examples" / "sample.ssn"

# VALIDATION.md §3
EXPECTED_Q = {"P1": 3.542510, "P2": 6.787155, "P3": 8.476773}
TOL = 5e-6


def test_constants_match_the_engine():
    assert ss.K_MANNING_US == 1.49
    assert ss.K_MANNING_SI == 1.0
    assert ss.G_US == 32.2
    assert ss.G_SI == 9.81


def test_intensity():
    # VALIDATION.md §1
    assert ss.intensity(60, 10, 0.8, 10) == pytest.approx(5.461693, abs=TOL)
    assert ss.intensity(60, 10, 0.8, 12) == pytest.approx(5.060729, abs=TOL)


def test_rational_q():
    i = ss.intensity(60, 10, 0.8, 12)
    assert ss.rational_q(0.70, i, 1.0) == pytest.approx(3.542510, abs=TOL)


def test_manning_capacity():
    # VALIDATION.md §2
    assert ss.manning_capacity(1.25, 0.013, 0.005) == pytest.approx(4.580060, abs=TOL)
    assert ss.manning_capacity(1.50, 0.013, 0.0052) == pytest.approx(7.595176, abs=TOL)
    assert ss.manning_capacity(1.75, 0.013, 2.0 / 300.0) == pytest.approx(12.972250, abs=TOL)


def test_normal_depth_and_geometry():
    # VALIDATION.md §5
    y = ss.normal_depth(3.542510, 1.25, 0.013, 0.005)
    assert y == pytest.approx(0.825392, abs=TOL)
    g = ss.circular_geometry(1.25, y)
    assert g["area"] == pytest.approx(0.859722, abs=TOL)
    assert 3.542510 / g["area"] == pytest.approx(4.120529, abs=TOL)


def test_si_switch_changes_the_manning_factor():
    us = ss.manning_capacity(1.0, 0.013, 0.01)
    si = ss.manning_capacity(1.0, 0.013, 0.01, si=True)
    assert us == pytest.approx(si * ss.K_MANNING_US, rel=1e-12)


def test_analyze_ssn_matches_validation_document():
    result = ss.analyze_ssn(SAMPLE.read_text())
    got = {p["id"]: p["design_q"] for p in result["pipes"]}
    for pipe, want in EXPECTED_Q.items():
        assert got[pipe] == pytest.approx(want, abs=TOL), pipe

    # VALIDATION.md §6: junction loss at N2, and nothing floods.
    p2 = next(p for p in result["pipes"] if p["id"] == "P2")
    assert p2["hgl_up"] == pytest.approx(103.789256, abs=TOL)
    assert not any(n["floods"] for n in result["nodes"])


def test_analyze_project_merges_catchments_like_the_app():
    # The demo project carries catchment C1, so its flows exceed the
    # catchment-free reference — the same numbers the desktop app shows.
    result = ss.analyze_project(ss.demo_project_json())
    p3 = next(p for p in result["pipes"] if p["id"] == "P3")
    assert p3["design_q"] == pytest.approx(8.663, abs=0.01)
    assert p3["design_q"] > EXPECTED_Q["P3"]


def test_pipe_and_node_dicts_carry_the_documented_keys():
    result = ss.analyze_ssn(SAMPLE.read_text())
    pipe_keys = {
        "id", "from", "to", "slope", "total_ca", "tc", "travel_time",
        "intensity", "design_q", "capacity", "pct_full", "velocity",
        "normal_depth", "critical_depth", "hgl_up", "hgl_dn", "surcharged",
    }
    node_keys = {"id", "tc", "rim", "hgl", "freeboard", "floods"}
    assert pipe_keys <= set(result["pipes"][0])
    assert node_keys <= set(result["nodes"][0])


@pytest.mark.parametrize(
    "call",
    [
        lambda: ss.manning_capacity(-1.0, 0.013, 0.005),
        lambda: ss.manning_capacity(1.25, 0.0, 0.005),
        lambda: ss.normal_depth(1.0, 0.0, 0.013, 0.005),
        lambda: ss.critical_depth(1.0, -1.0),
        lambda: ss.analyze_project("{not json"),
        lambda: ss.analyze_ssn("NOT A NETWORK"),
    ],
)
def test_bad_input_raises_instead_of_crashing(call):
    with pytest.raises(ValueError):
        call()


def test_missing_file_raises_oserror():
    with pytest.raises(OSError):
        ss.analyze_file("this-file-does-not-exist.ssproj")
