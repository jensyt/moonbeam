#!/usr/bin/env python3
import os
import re
import sys

def bump_version(old_version, op):
    # Parse version (e.g. 0.7.0 -> [0, 7, 0])
    parts = list(map(int, old_version.split('.')))
    while len(parts) < 3:
        parts.append(0)
    
    op_lower = op.lower()
    if op_lower == 'major':
        parts[0] += 1
        parts[1] = 0
        parts[2] = 0
    elif op_lower == 'minor':
        parts[1] += 1
        parts[2] = 0
    elif op_lower == 'patch':
        parts[2] += 1
    else:
        # Assume op is a specific version string like "0.7.1" or "0.8"
        new_parts = list(map(int, op.split('.')))
        while len(new_parts) < 3:
            new_parts.append(0)
        parts = new_parts
        
    return f"{parts[0]}.{parts[1]}.{parts[2]}"

def update_crate_version(cargo_toml_path, op):
    with open(cargo_toml_path, 'r', encoding='utf-8') as f:
        content = f.read()
    
    # Find the [package] section and the version key
    match = re.search(r'(?s)(\[package\].*?\bversion\s*=\s*")([^"]+)(")', content)
    if not match:
        raise ValueError(f"Could not find package version in {cargo_toml_path}")
        
    old_version = match.group(2)
    new_version = bump_version(old_version, op)
    
    # Replace the version
    new_content = content[:match.start(2)] + new_version + content[match.end(2):]
    
    with open(cargo_toml_path, 'w', encoding='utf-8') as f:
        f.write(new_content)
        
    return old_version, new_version

def update_dependencies(cargo_toml_path, package_name, new_version):
    with open(cargo_toml_path, 'r', encoding='utf-8') as f:
        content = f.read()
        
    modified = False
    
    # Helper to format new version based on old version digit count
    def get_replacement_version(old_v):
        old_digits = len(old_v.split('.'))
        new_parts = new_version.split('.')
        if old_digits == 2:
            return f"{new_parts[0]}.{new_parts[1]}"
        return f"{new_parts[0]}.{new_parts[1]}.{new_parts[2]}"
        
    # Replace inline table dependencies
    # E.g. moonbeam-attributes = { path = "../moonbeam-attributes", version = "0.4" }
    pattern_table = re.compile(rf'({re.escape(package_name)}\s*=\s*\{{[^}}]*?\bversion\s*=\s*")([^"]+)(")')
    def repl_table(m):
        nonlocal modified
        old_v = m.group(2)
        new_v = get_replacement_version(old_v)
        if old_v != new_v:
            modified = True
            return m.group(1) + new_v + m.group(3)
        return m.group(0)
        
    content = pattern_table.sub(repl_table, content)
    
    # Replace simple string dependencies
    # E.g. moonbeam = "0.7"
    pattern_str = re.compile(rf'(?m)^(\s*{re.escape(package_name)}\s*=\s*")([^"]+)(")')
    def repl_str(m):
        nonlocal modified
        old_v = m.group(2)
        new_v = get_replacement_version(old_v)
        if old_v != new_v:
            modified = True
            return m.group(1) + new_v + m.group(3)
        return m.group(0)
        
    content = pattern_str.sub(repl_str, content)
    
    if modified:
        with open(cargo_toml_path, 'w', encoding='utf-8') as f:
            f.write(content)

def update_file_references(file_path, package_name, old_version, new_version):
    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()
        
    old_parts = old_version.split('.')
    old_v2 = f"{old_parts[0]}.{old_parts[1]}"
    old_v3 = f"{old_parts[0]}.{old_parts[1]}.{old_parts[2]}"
    
    new_parts = new_version.split('.')
    new_v2 = f"{new_parts[0]}.{new_parts[1]}"
    new_v3 = f"{new_parts[0]}.{new_parts[1]}.{new_parts[2]}"
    
    modified = False
    
    # 1. package_name = "X.Y" or package_name = "X.Y.Z"
    pattern_eq = re.compile(rf'({re.escape(package_name)}\s*=\s*")({re.escape(old_v2)}|{re.escape(old_v3)})(")')
    def repl_eq(m):
        nonlocal modified
        matched_v = m.group(2)
        new_v = new_v2 if len(matched_v.split('.')) == 2 else new_v3
        if matched_v != new_v:
            modified = True
            return m.group(1) + new_v + m.group(3)
        return m.group(0)
    content = pattern_eq.sub(repl_eq, content)
    
    # 2. package_name/X.Y or package_name/X.Y.Z (specifically for Server headers)
    pattern_slash = re.compile(rf'({re.escape(package_name)}/)({re.escape(old_v2)}|{re.escape(old_v3)})\b')
    def repl_slash(m):
        nonlocal modified
        matched_v = m.group(2)
        new_v = new_v2 if len(matched_v.split('.')) == 2 else new_v3
        if matched_v != new_v:
            modified = True
            return m.group(1) + new_v
        return m.group(0)
    content = pattern_slash.sub(repl_slash, content)
    
    if modified:
        with open(file_path, 'w', encoding='utf-8') as f:
            f.write(content)

def main():
    if len(sys.argv) < 3 or (len(sys.argv) - 1) % 2 != 0:
        print("Usage: python3 update_version.py <package_name> <operation> [<package_name> <operation> ...]")
        print("Operations: major, minor, patch, or exact version (e.g. 0.7.1)")
        sys.exit(1)
        
    script_dir = os.path.dirname(os.path.abspath(__file__))
    workspace_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(script_dir))))
    
    # Map packages to their directory names relative to workspace root
    package_dirs = {
        "moonbeam": "moonbeam",
        "moonbeam-attributes": "moonbeam-attributes",
        "moonbeam-serde": "moonbeam-serde",
        "moonbeam-forms": "moonbeam-forms",
    }
    
    updates = []
    for i in range(1, len(sys.argv), 2):
        pkg = sys.argv[i]
        op = sys.argv[i+1]
        if pkg not in package_dirs:
            print(f"Error: Unknown package '{pkg}'")
            sys.exit(1)
        updates.append((pkg, op))
        
    # Get all Cargo.toml and .md files in workspace
    all_cargo_tomls = []
    all_md_files = []
    
    for root, dirs, files in os.walk(workspace_root):
        # Skip target and hidden dirs
        dirs[:] = [d for d in dirs if d not in ('target', 'node_modules') and not d.startswith('.')]
        for f in files:
            path = os.path.join(root, f)
            if f == 'Cargo.toml':
                all_cargo_tomls.append(path)
            elif f.endswith('.md'):
                all_md_files.append(path)
                
    # Also add the specific test file in moonbeam/src/server/mod.rs
    server_mod_path = os.path.join(workspace_root, "moonbeam", "src", "server", "mod.rs")
    
    for pkg, op in updates:
        cargo_path = os.path.join(workspace_root, package_dirs[pkg], "Cargo.toml")
        
        try:
            old_version, new_version = update_crate_version(cargo_path, op)
            print(f"Updated {pkg} in {package_dirs[pkg]}/Cargo.toml from {old_version} to {new_version}")
            
            # Update dependency version in other Cargo.toml files
            for tom_path in all_cargo_tomls:
                update_dependencies(tom_path, pkg, new_version)
                
            # Update documentation and markdown files
            for md_path in all_md_files:
                update_file_references(md_path, pkg, old_version, new_version)
                
            # Update references in server/mod.rs test
            if os.path.exists(server_mod_path):
                update_file_references(server_mod_path, pkg, old_version, new_version)
                
        except Exception as e:
            print(f"Error updating {pkg}: {e}")
            sys.exit(1)
            
    print("Version updates completed successfully!")

if __name__ == "__main__":
    main()
