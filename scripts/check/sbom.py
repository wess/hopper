#!/usr/bin/env python3
"""Emit the locked Rust dependency graph and bundled helper identities.

The guest OS's transitive packages are outside this host SBOM's scope. They
are identified separately by the immutable image digest in the VM profile.
"""
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
ALLOWED = {"MIT", "MIT-0", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "MPL-2.0", "Unicode-3.0", "CC0-1.0", "Unlicense", "NCSA", "BSL-1.0", "0BSD"}


def acceptable(expression):
    tokens = re.findall(r"\(|\)|[A-Za-z0-9.+-]+", expression)
    at = 0

    def atom():
        nonlocal at
        if at >= len(tokens):
            raise ValueError("Incomplete license expression")
        word = tokens[at]
        at += 1
        if word == "(":
            value = either()
            if at >= len(tokens) or tokens[at] != ")":
                raise ValueError("Unclosed license expression")
            at += 1
        else:
            value = word in ALLOWED
            if at < len(tokens) and tokens[at] == "WITH":
                at += 1
                if at >= len(tokens):
                    raise ValueError("Missing license exception")
                value = value and word == "Apache-2.0" and tokens[at] == "LLVM-exception"
                at += 1
        return value

    def both():
        nonlocal at
        value = atom()
        while at < len(tokens) and tokens[at] == "AND":
            at += 1
            other = atom()
            value = value and other
        return value

    def either():
        nonlocal at
        value = both()
        while at < len(tokens) and tokens[at] == "OR":
            at += 1
            other = both()
            value = value or other
        return value

    result = either()
    if at != len(tokens):
        raise ValueError("Unsupported license expression")
    return result


def main():
    output = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / "dist")
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT))
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    checksums = {(p["name"], p["version"], p.get("source")): p["checksum"] for p in lock["package"] if "checksum" in p}
    components, references, notices = [], {}, []
    for package in metadata["packages"]:
        expression = (package["license"] or "").replace("/", " OR ")
        if not expression or not acceptable(expression):
            raise SystemExit(f"License review required: {package['name']} {package['version']}: {expression!r}")
        ref = "rust:" + hashlib.sha256(package["id"].encode()).hexdigest()
        references[package["id"]] = ref
        component = {"type": "library", "bom-ref": ref, "name": package["name"], "version": package["version"], "licenses": [{"expression": expression}]}
        checksum = checksums.get((package["name"], package["version"], package["source"]))
        if checksum:
            component["hashes"] = [{"alg": "SHA-256", "content": checksum}]
        if package["source"] and package["source"].startswith("registry+"):
            component["purl"] = f"pkg:cargo/{package['name']}@{package['version']}"
        if package["repository"]:
            component["externalReferences"] = [{"type": "vcs", "url": package["repository"]}]
        components.append(component)
        notices.append(f"{package['name']} {package['version']} | {expression} | {package['repository'] or ''}")
    dependencies = [{"ref": references[node["id"]], "dependsOn": sorted(references[dep] for dep in node["dependencies"])} for node in metadata["resolve"]["nodes"]]
    for name in ["lima", "docker", "compose", "buildx", "credential-helper"]:
        script = (ROOT / "scripts" / "build" / f"{name}.sh").read_text()
        version = re.search(r'^(?:version|ver)="([^"]+)"', script, re.M).group(1)
        digest = re.search(r'^sha256="([0-9a-f]{64})"', script, re.M).group(1)
        components.append({"type": "application", "bom-ref": "helper:" + name, "name": name, "version": version, "hashes": [{"alg": "SHA-256", "content": digest}], "properties": [{"name": "hopper:hashScope", "value": "upstream download before packaging and signing"}]})
    version = next(p["version"] for p in metadata["packages"] if p["name"] == "app" and p["id"] in metadata["workspace_members"])
    dependencies.append({"ref": "hopper", "dependsOn": sorted(references[p] for p in metadata["workspace_members"]) + ["helper:" + name for name in ["lima", "docker", "compose", "buildx", "credential-helper"]]})
    document = {"bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1, "metadata": {"component": {"type": "application", "bom-ref": "hopper", "name": "Hopper", "version": version}}, "components": components, "dependencies": dependencies}
    output.mkdir(parents=True, exist_ok=True)
    (output / "hopper.cdx.json").write_text(json.dumps(document, indent=2) + "\n")
    (output / "dependencies.txt").write_text("Locked Rust dependency license declarations; upstream license texts remain authoritative.\n\n" + "\n".join(sorted(notices)) + "\n")
    print(f"Wrote {len(components)} components and license declarations to {output}")


if __name__ == "__main__":
    main()
