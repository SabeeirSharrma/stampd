"""Shared test configuration — must run before any test module imports app."""

import os
import tempfile

# Set the temp DB path BEFORE app.main or app.database are imported
_tmp = tempfile.NamedTemporaryFile(suffix=".db", delete=False)
_tmp.close()
os.environ["STAMPD_DB_PATH"] = _tmp.name
