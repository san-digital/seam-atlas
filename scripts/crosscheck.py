#!/usr/bin/env python3
"""Independent Python cross-check for seam-atlas CLI output against direct rule evaluation."""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys


def eval_clinical_v1(inp: dict) -> str:
    scales = {"mcg": 1, "mg": 1000, "g": 1000000}
    scaled = inp["dose"] * scales[inp["unit"]]
    if scaled >= 4_000_000:
        return "Emergency"
    if inp["age_years"] == 0 and scaled > 60_000:
        return "Escalate"
    return "Routine"


def eval_iam_v1(inp: dict) -> str:
    if inp["action"] in ("write", "delete") and inp["payload_bytes"] > 10_485_760:
        return "Deny"
    if inp["action"] == "delete" and inp["ip_zone"] == "public":
        return "Deny"
    if inp["env"] == "prod" and inp["mfa"] is False:
        return "Deny"
    if inp["role"] == "admin" and inp["ip_zone"] in ("corp", "vpn"):
        return "Allow"
    if inp["role"] == "auditor" and inp["action"] == "read":
        return "Allow"
    if inp["role"] == "developer" and inp["env"] in ("dev", "staging") and inp["action"] in ("read", "write"):
        return "Allow"
    return "Deny"


def eval_iam_v2(inp: dict) -> str:
    if inp["action"] in ("write", "delete") and inp["payload_bytes"] > 10_485_760:
        return "Deny"
    if inp["role"] == "auditor" and inp["action"] == "read":
        return "Allow"
    if inp["action"] == "delete" and inp["ip_zone"] == "public":
        return "Deny"
    if inp["env"] == "prod" and inp["mfa"] is False:
        return "Deny"
    if inp["role"] == "admin" and inp["ip_zone"] in ("corp", "vpn"):
        return "Allow"
    if inp["role"] == "developer" and inp["env"] in ("dev", "staging") and inp["action"] in ("read", "write"):
        return "Allow"
    return "Deny"


def main() -> int:
    repo = pathlib.Path(__file__).resolve().parent.parent
    bin_cmd = ["cargo", "run", "--release", "--quiet", "--bin", "seam-atlas", "--"]

    cliffs_raw = subprocess.check_output(
        bin_cmd + ["cliffs", "--json", str(repo / "policies/clinical_v1.policy")],
        cwd=repo,
        text=True,
    )
    cliffs = json.loads(cliffs_raw)["cliff_edges"]
    assert len(cliffs) >= 4
    for edge in cliffs:
        a = edge["input_a"]
        b = edge["input_b"]
        diffs = [k for k in a if a[k] != b[k]]
        assert len(diffs) == 1 and diffs[0] == edge["field"]
        assert eval_clinical_v1(a) == edge["outcome_a"]["label"]
        assert eval_clinical_v1(b) == edge["outcome_b"]["label"]
        assert eval_clinical_v1(a) != eval_clinical_v1(b)

    diff_raw = subprocess.check_output(
        bin_cmd
        + [
            "diff",
            "--json",
            str(repo / "policies/iam_v1.policy"),
            str(repo / "policies/iam_v2.policy"),
        ],
        cwd=repo,
        text=True,
    )
    diff = json.loads(diff_raw)
    assert diff["is_equivalent"] is False
    assert int(diff["total_volume"]) == 22_649_241_816
    assert int(diff["changed_volume"]) == 314_572_803
    assert len(diff["classes"]) == 1
    w = diff["classes"][0]["witness"]
    assert eval_iam_v1(w) == "Deny"
    assert eval_iam_v2(w) == "Allow"

    print("crosscheck: ok (clinical cliffs and 22.6B-state IAM diff verified independently)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
