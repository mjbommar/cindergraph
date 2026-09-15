"""Public serialization must not depend on Python's process hash seed."""

import json
import os
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]
WORKER = r"""
import hashlib
import json
from pathlib import Path
import sys
import cindergraph as cg

root = Path(sys.argv[1]) / "crates/cindergraph/tests"
files = sorted((root / "decompiler_fixtures/src").glob("*.c")) + sorted(
    (root / "decbench_corpus/src").glob("*.c")
)
assert len(files) == 210
digest = hashlib.sha256()
for path in files:
    code = path.read_text(encoding="utf-8")
    values = [cg.analyze(code).to_dict(), cg.data_flow(code),
              cg.call_summaries(code), cg.control_dependence(code)]
    for representation in ("ast", "cfg", "ddg", "cdg", "pdg"):
        for format in ("json", "graphml", "dot", "mermaid"):
            values.append(cg.export_graphs(code, repr=representation, format=format))
    # Do not sort keys: dictionary insertion order is part of what is tested.
    payload = json.dumps(values, ensure_ascii=True, separators=(",", ":")).encode()
    digest.update(len(payload).to_bytes(8, "little"))
    digest.update(payload)
print(json.dumps(dict(digest=digest.hexdigest(), hash_probe=hash("cindergraph"), files=len(files))))
"""


def test_public_outputs_match_across_hash_seeds(tmp_path: Path) -> None:
    results = []
    for seed in ("0", "1", "42"):
        env = dict(os.environ, PYTHONHASHSEED=seed, PYTHONPATH=str(ROOT / "python"))
        result = subprocess.run(
            [sys.executable, "-c", WORKER, str(ROOT)],
            cwd=tmp_path,
            env=env,
            capture_output=True,
            text=True,
            # The worker deliberately rebuilds 4 analyses and 20 graph
            # serializations for each of 210 files. Keep a real hang bound,
            # but leave headroom above the measured roughly 30-second local
            # runtime and slower shared CI runners now that data-flow output
            # includes memory-region records.
            timeout=90,
            check=True,
        )
        results.append(json.loads(result.stdout))
    assert len({r["hash_probe"] for r in results}) == 3, "hash seeds were not effective"
    assert len({r["digest"] for r in results}) == 1, results
    print(results)
