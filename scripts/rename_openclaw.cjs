const fs = require('fs');
const path = require('path');

const TARGET_EXTS = ['.ts', '.js', '.mjs', '.rs', '.py', '.toml', '.json', '.md', '.yml', '.yaml', '.sh'];
const IGNORE_DIRS = ['node_modules', 'target', '.git', 'generated', 'openclaw']; 

const rootDir = 'c:\\Users\\dixon\\Desktop\\Open-code';

function walkDir(dir, callback) {
  const files = fs.readdirSync(dir);
  for (const file of files) {
    if (IGNORE_DIRS.includes(file)) continue;
    const fullPath = path.join(dir, file);
    const stat = fs.statSync(fullPath);
    if (stat.isDirectory()) {
      walkDir(fullPath, callback);
    } else {
      if (TARGET_EXTS.includes(path.extname(fullPath)) || fullPath.endsWith('install.sh') || fullPath.endsWith('search.mjs') || path.basename(fullPath) === 'install.js' || path.basename(fullPath) === 'PRODUCTION.md' || path.basename(fullPath) === 'AGENT.md') {
        callback(fullPath);
      }
    }
  }
}

let modifiedFiles = 0;

walkDir(rootDir, (filePath) => {
  // Never modify lockfiles or claude settings
  if (filePath.endsWith('package-lock.json') || filePath.includes('.claude')) return;

  let content = fs.readFileSync(filePath, 'utf-8');
  let original = content;

  // 1. Explicitly do package scopes
  content = content.replace(/@openclaw\//g, '@claw/');

  // 2. Explicitly do python package namespace (if not part of a path)
  content = content.replace(/(?<!\/|\\)openclaw_sdk(?!\/|\\)/g, 'claw_sdk');

  // 3. Explicitly do node package namespace (if not part of a path)
  content = content.replace(/(?<!\/|\\)openclaw-sdk(?!\/|\\)/g, 'claw-sdk');

  // 4. OPENCLAW env vars, etc
  content = content.replace(/OPENCLAW(?![-_]GATEWAY)/g, 'CLAW');

  // 5. OpenClaw (brand name)
  content = content.replace(/OpenClaw(?![-_][Gg]ateway|\/|\\)/g, 'Claw');

  // 6. Generic openclaw 
  content = content.replace(/(?<![A-Za-z0-9])openclaw(?![-_]gateway|\/|\\)(?![a-zA-Z0-9])/g, (match, offset, string) => {
    const prevChar = offset > 0 ? string[offset - 1] : '';
    const nextChar = string[offset + match.length];
    
    // Avoid replacing inside paths or urls
    // If it is preceded by a slash, it's highly likely part of a directory path like `packages/openclaw-sdk`
    if (prevChar === '/' || prevChar === '\\') {
       return match;
    }
    
    return 'claw';
  });

  if (original !== content) {
    fs.writeFileSync(filePath, content, 'utf-8');
    modifiedFiles++;
    console.log('Modified:', filePath);
  }
});

console.log('Total files modified:', modifiedFiles);
