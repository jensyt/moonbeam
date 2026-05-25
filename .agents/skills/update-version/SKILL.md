---
name: update-version
description: Update the version of a package in the Moonbeam repository. Use this to update package versions to target numbers.
compatibility: Requires Python 3
---

# Update Version

Update the version of the specified packages to the given version numbers or semver bump operators (e.g. `major`, `minor`, `patch`).

## Workflow

1. **Verify Target Packages and Operators**: Extract package names and desired operations from the user's prompt (e.g. "moonbeam minor" or "moonbeam-serde 0.4.0").
2. **Execute the Python Script**: Run the automation script to perform the version bump and reference updates:
   ```bash
   python3 scripts/update_version.py [package_name] [operation] ...
   ```
   For example, if the user requested updating `moonbeam` to the next minor version and `moonbeam-serde` to the next major version:
   ```bash
   python3 scripts/update_version.py moonbeam minor moonbeam-serde major
   ```
3. **Validation**: Run Cargo verification command to ensure everything is correct:
   ```bash
   cargo check --workspace --all-features
   ```
