#!/usr/bin/env python3

import json
import os
import sys

# Permissive license list
PERMISSIVE_LICENSES = {
    "MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
    "Unicode-3.0", "Unicode-DFS-2016", "CC0-1.0", "0BSD", "BSL-1.0", "MIT-0",
    "Unlicense"
}

# REVIEW licenses (treated specially)
REVIEW_LICENSES = {
    "MPL-2.0+", "OFL-1.1", "Ubuntu-font-1.0", "CDLA-Permissive-2.0"
}


def classify(expr: str) -> str:
    """
    Classify a license expression according to the requirements:
    1. Treat "/" as OR
    2. Parse parentheses: (A OR B) AND C means both sides of AND must be permissive
    3. "X WITH Y-exception": classify by X
    4. Treat "MPL-2.0+" as REVIEW
    5. Treat OFL-1.1, Ubuntu-font-1.0 and CDLA-Permissive-2.0 as REVIEW with "font or data license"
    6. An empty license string is UNKNOWN (REVIEW)
    """
    if not expr:
        return "REVIEW"
    
    # Normalize the expression by replacing / with OR
    expr = expr.replace('/', ' OR ')
    
    # Handle WITH exceptions - extract the base license
    if ' WITH ' in expr:
        # Split by WITH and take the first part
        parts = expr.split(' WITH ')
        base_license = parts[0].strip()
        # If base license is in REVIEW_LICENSES, return REVIEW
        if base_license in REVIEW_LICENSES:
            return "REVIEW"
        # If base license is permissive, return PERMISSIVE
        if base_license in PERMISSIVE_LICENSES:
            return "PERMISSIVE"
        # Otherwise check if it's in our known list
        return classify(base_license)
    
    # Handle parentheses: split by AND and check each part
    if ' AND ' in expr:
        parts = expr.split(' AND ')
        for part in parts:
            part = part.strip()
            # If part has parentheses, remove them for checking
            if part.startswith('(') and part.endswith(')'):
                part = part[1:-1].strip()
            # If any part is not permissive, return REVIEW
            if not is_permissive_license(part):
                return "REVIEW"
        return "PERMISSIVE"
    
    # Handle OR: at least one part must be permissive
    if ' OR ' in expr:
        parts = expr.split(' OR ')
        for part in parts:
            part = part.strip()
            # If part has parentheses, remove them for checking
            if part.startswith('(') and part.endswith(')'):
                part = part[1:-1].strip()
            if is_permissive_license(part):
                return "PERMISSIVE"
        return "REVIEW"
    
    # Simple case: single license
    return "PERMISSIVE" if is_permissive_license(expr) else "REVIEW"


def is_permissive_license(license_str):
    """Check if a license string is permissive."""
    if not license_str:
        return False
    
    # Handle AND logic: all parts must be permissive
    if " AND " in license_str:
        parts = license_str.split(" AND ")
        return all(is_permissive_license(part.strip()) for part in parts)
    
    # Handle OR logic: at least one part must be permissive
    if " OR " in license_str:
        parts = license_str.split(" OR ")
        return any(is_permissive_license(part.strip()) for part in parts)
    
    # Handle WITH exceptions - extract the base license
    if ' WITH ' in license_str:
        parts = license_str.split(' WITH ')
        base_license = parts[0].strip()
        return is_permissive_license(base_license)
    
    # Simple case: single license
    return license_str.strip() in PERMISSIVE_LICENSES


def classify_license(license_str):
    """Classify a license as PERMISSIVE or REVIEW."""
    if not license_str:
        return "REVIEW"  # UNKNOWN license
    result = classify(license_str)
    return result


def run_tests():
    """Run self-test cases"""
    test_cases = [
        ("MIT/Apache-2.0", "PERMISSIVE"),
        ("Apache-2.0 / MIT", "PERMISSIVE"),
        ("Unlicense/MIT", "PERMISSIVE"),
        ("Zlib AND (MIT OR Apache-2.0)", "PERMISSIVE"),
        ("(MIT OR Apache-2.0) AND Unicode-3.0", "PERMISSIVE"),
        ("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT", "PERMISSIVE"),
        ("MPL-2.0+", "REVIEW"),
        ("(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0", "REVIEW"),
        ("", "REVIEW")
    ]
    
    all_passed = True
    for i, (expr, expected) in enumerate(test_cases):
        result = classify(expr)
        if result != expected:
            print(f"Test {i+1} failed: '{expr}' -> got '{result}', expected '{expected}'")
            all_passed = False
        else:
            print(f"Test {i+1} passed: '{expr}' -> {result}")
    
    if all_passed:
        print("All tests passed!")
    else:
        print("Some tests failed.")
    
    return all_passed


def main():
    # Check if --test flag is provided
    if len(sys.argv) > 1 and sys.argv[1] == "--test":
        run_tests()
        return
    
    # Read the metadata JSON
    with open('/tmp/meta.json', 'r') as f:
        meta = json.load(f)
    
    # Create a mapping of package names to their info
    packages = {}
    
    # Get workspace members
    workspace_members = set()
    if 'workspace_members' in meta:
        for member in meta['workspace_members']:
            # Extract package name from the member ID
            package_name = member.split('#')[0] if '#' in member else member
            workspace_members.add(package_name)
    
    # Process packages
    for package in meta.get('packages', []):
        name = package['name']
        version = package['version']
        license = package.get('license', '')
        license_file = package.get('license_file', '')
        
        # Skip workspace members
        if name in workspace_members:
            continue
            
        # If license is empty, use license_file or mark as UNKNOWN
        if not license:
            if license_file:
                license = f"UNKNOWN (file: {license_file})"
            else:
                license = "UNKNOWN"
        
        packages[name] = {
            'name': name,
            'version': version,
            'license': license,
            'license_class': classify_license(license),
            'dependents': set()
        }
    
    # Build dependency graph
    for package in meta.get('packages', []):
        name = package['name']
        
        # Skip workspace members
        if name in workspace_members:
            continue
            
        # Process dependencies
        for dep in package.get('dependencies', []):
            dep_name = dep['name']
            # Skip if dependency is a workspace member
            if dep_name in workspace_members:
                continue
            # Add to dependents if the dependency exists in our packages
            if dep_name in packages:
                packages[dep_name]['dependents'].add(name)
    
    # Prepare data for output
    all_packages = list(packages.values())
    review_packages = [pkg for pkg in all_packages if pkg['license_class'] == 'REVIEW']
    permissive_packages = [pkg for pkg in all_packages if pkg['license_class'] == 'PERMISSIVE']
    
    # Sort packages by name
    all_packages.sort(key=lambda x: x['name'])
    review_packages.sort(key=lambda x: x['name'])
    
    # Create output directory if it doesn't exist
    os.makedirs('docs/research/licences', exist_ok=True)
    
    # Write the markdown report
    with open('docs/research/licences/rust-deps.md', 'w') as f:
        # Summary line
        total_count = len(all_packages)
        permissive_count = len(permissive_packages)
        review_count = len(review_packages)
        f.write(f"# Rust Dependency Licenses\n\n")
        f.write(f"Total packages: {total_count}\n")
        f.write(f"Permissive licenses: {permissive_count}\n")
        f.write(f"Review required: {review_count}\n\n")
        
        # Review packages table
        f.write("## Packages Requiring Review\n\n")
        f.write("| Package | Version | License | Dependent Crates |\n")
        f.write("|--------|---------|---------|------------------|\n")
        for pkg in review_packages:
            dependents_str = ", ".join(sorted(pkg['dependents'])) if pkg['dependents'] else "None"
            f.write(f"| {pkg['name']} | {pkg['version']} | {pkg['license']} | {dependents_str} |\n")
        
        f.write("\n## All Packages\n\n")
        f.write("| Package | Version | License | License Class | Dependent Crates |\n")
        f.write("|--------|---------|---------|---------------|------------------|\n")
        for pkg in all_packages:
            dependents_str = ", ".join(sorted(pkg['dependents'])) if pkg['dependents'] else "None"
            f.write(f"| {pkg['name']} | {pkg['version']} | {pkg['license']} | {pkg['license_class']} | {dependents_str} |\n")

if __name__ == "__main__":
    main()