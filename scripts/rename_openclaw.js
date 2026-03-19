const fs = require('fs');
const path = require('path');

const TARGET_EXTS = ['.ts', '.js', '.mjs', '.rs', '.py', '.toml', '.json', '.md', '.yml', '.yaml'];
const IGNORE_DIRS = ['node_modules', 'target', '.git', 'generated', 'claw']; // note: ignoring the claw submodule entirely

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
      if (TARGET_EXTS.includes(path.extname(fullPath))) {
        callback(fullPath);
      }
    }
  }
}

let modifiedFiles = 0;

walkDir(rootDir, (filePath) => {
  // Read file
  let content = fs.readFileSync(filePath, 'utf-8');
  let original = content;

  // We need to replace Claw, claw, CLAW
  // EXCEPT: openclaw-gateway, openclaw/ (path)
  
  // Replace CLAW (check negative lookaheads)
  content = content.replace(/CLAW(?![-_]GATEWAY)/g, 'CLAW');
  
  // Replace Claw
  content = content.replace(/Claw(?![-_][Gg]ateway|\/)/g, 'Claw');
  
  // Replace claw
  content = content.replace(/claw(?![-_]gateway|\/|\\|-[a-zA-Z0-9]+-)/g, (match, offset, string) => {
    // If it's part of a path like `/claw` or `./claw` we should be careful.
    // Let's check the preceding character.
    const prevChar = offset > 0 ? string[offset - 1] : '';
    const nextChar = string[offset + match.length];
    
    // If it's in a path like 'openclaw/ui', nextChar is '/' or '\'
    if (nextChar === '/' || nextChar === '\\') {
      return match; // skip directory reference `openclaw/`
    }
    
    if (prevChar === '/' || prevChar === '\\') {
       // it's `.../claw`
       // if next char is boundary (like quote), it represents the folder.
       if (nextChar === '"' || nextChar === "'" || nextChar === '`') return match;
    }
    
    // We want to replace `claw-sdk` but not `openclaw-gateway`.
    // The regex negative lookahead (?![-_]gateway) already guards against gateway.
    return 'claw';
  });

  if (original !== content) {
    fs.writeFileSync(filePath, content, 'utf-8');
    modifiedFiles++;
    console.log('Modified:', filePath);
  }
});

console.log('Total files modified:', modifiedFiles);
