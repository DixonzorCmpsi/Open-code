import os
import re

RULES = [
    (r'\bopenclaw\b', 'claw', 'User-facing strings (lower)'),
    (r'\bOpenClaw\b', 'Claw', 'User-facing strings (upper)'),
    (r'\bOPENCLAW_[A-Z0-Z_]+', 'CLAW_*', 'Env vars'),
    (r'x-openclaw-key', 'x-claw-key', 'HTTP headers'),
    (r'\.openclaw\b', '.claw', 'State directory'),
    (r'\bopenclaw\.json\b', 'claw.json', 'Config file'),
    (r'\[openclaw-gateway\]', '[claw-gateway]', 'Log prefixes'),
    (r'\bopenclaw-[a-z0-9]+', 'claw-*', 'Temp dir prefixes'),
    (r'\bopenclaw:', 'claw:', 'Redis namespaces'),
]

EXCLUDES = [
    'openclaw-gateway', 
    'OpenClawConfig', 'OpenClawCliError', 'OpenClawClient',
    '@openclaw/sdk', 'openclaw_sdk', '@openclaw/tools.browser',
    'OPENCLAW_AST_HASH',
]

TARGETS = [
    'src', 'openclaw-gateway', 'specs'
]
FILES = [
    '.env.example', '.gitignore', 'package.json', 'Cargo.toml',
    'README.md', 'PRODUCTION.md', 'QUICKSTART.md', 'AGENT.md'
]

def scan_file(filepath):
    results = []
    with open(filepath, 'r', encoding='utf-8', errors='ignore') as f:
        for line_num, line in enumerate(f, 1):
            if any(ex in line for ex in EXCLUDES):
                # We do a naive check: if the match is exactly one of the excludes, we might skip.
                # Actually let's just do regex replacement and see if anything changes EXCEPT excludes.
                pass
            
            # Simple match
            for pat, repl, rule_name in RULES:
                matches = re.finditer(pat, line)
                for m in matches:
                    val = m.group(0)
                    if val in EXCLUDES or any(val in ex for ex in EXCLUDES) or any(ex in val for ex in EXCLUDES):
                        continue
                    # specifically check OPENCLAW_AST_HASH
                    if 'OPENCLAW_AST_HASH' in val:
                        continue
                    if 'openclaw-gateway' in val:
                        if pat != r'\[openclaw-gateway\]':
                            continue
                            
                    results.append(f"{filepath}:{line_num} -> found '{val}' ({rule_name})")
    return results

def main():
    base = r'c:\Users\dixon\Desktop\Open-code'
    all_results = []
    
    # 1. src/ (.rs)
    for root, _, files in os.walk(os.path.join(base, 'src')):
        for f in files:
            if f.endswith('.rs'):
                all_results.extend(scan_file(os.path.join(root, f)))
                
    # 2. openclaw-gateway/ (*.ts, *.js, etc)
    for root, _, files in os.walk(os.path.join(base, 'openclaw-gateway')):
        if 'node_modules' in root: continue
        for f in files:
            if f.endswith(('.ts', '.mts', '.mjs', '.js')):
                all_results.extend(scan_file(os.path.join(root, f)))
                
    # 3. specs/
    for root, _, files in os.walk(os.path.join(base, 'specs')):
        for f in files:
            if f.endswith('.md'):
                all_results.extend(scan_file(os.path.join(root, f)))
                
    # 4. Root config files & markdown
    for f in FILES:
        p = os.path.join(base, f)
        if os.path.exists(p):
            all_results.extend(scan_file(p))
            
    print(f"Found {len(all_results)} violations:")
    for r in all_results:
        print(r)

if __name__ == '__main__':
    main()
