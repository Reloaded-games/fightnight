// Khronos validation of every packaged GLB, plus manifest integrity.
// npm install --prefix target/gltf-validation --no-save --package-lock=false gltf-validator
// node tools/assetgen/validate_glb.mjs
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const require = createRequire(import.meta.url);
const validator = require(path.join(root, 'target/gltf-validation/node_modules/gltf-validator'));
const output = path.join(root, 'assets/generated');
const manifest = JSON.parse(fs.readFileSync(path.join(output, 'manifest.json')));
const reports = [];
for (const item of manifest.files) {
  const bytes = fs.readFileSync(path.join(output, item.file));
  if (bytes.length !== item.bytes || crypto.createHash('sha256').update(bytes).digest('hex') !== item.sha256) {
    throw new Error(`Manifest mismatch: ${item.file}`);
  }
  const report = await validator.validateBytes(new Uint8Array(bytes), {
    uri: item.file, format: 'glb', maxIssues: 20, writeTimestamp: false,
  });
  reports.push({ file: item.file, ...report.issues });
  if (report.issues.numErrors || report.issues.numWarnings) {
    console.log(item.file, JSON.stringify(report.issues));
  }
}
const errors = reports.reduce((n, r) => n + r.numErrors, 0);
const warnings = reports.reduce((n, r) => n + r.numWarnings, 0);
fs.writeFileSync(path.join(root, 'target/asset-export/validation.json'), JSON.stringify({
  validator: validator.version(), files: reports.length, errors, warnings, reports,
}, null, 2));
fs.writeFileSync(path.join(output, 'validation.json'), JSON.stringify({
  validator: 'Khronos glTF Validator ' + validator.version(), files: reports.length,
  errors, warnings, manifest_sha256: crypto.createHash('sha256')
    .update(fs.readFileSync(path.join(output, 'manifest.json'))).digest('hex'),
}, null, 2) + '\n');
console.log(`Khronos glTF validator: ${reports.length} files, ${errors} errors, ${warnings} warnings.`);
if (errors) process.exitCode = 1;
