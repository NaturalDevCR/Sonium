#!/usr/bin/env node
/**
 * Single source of truth for Sonium versioning.
 *
 * Reads the version from the workspace Cargo.toml and syncs it to:
 *   - client-gui/src-tauri/tauri.conf.json
 *   - client-gui/package.json
 *   - client-gui/src/App.vue
 *   - web/package.json
 *   - web/src/views/ControlView.vue
 *
 * Run this script after changing the version in Cargo.toml and before building.
 */

const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');

function readCargoVersion() {
    const cargoToml = fs.readFileSync(path.join(ROOT, 'Cargo.toml'), 'utf8');
    const match = cargoToml.match(/^version\s*=\s*"([^"]+)"/m);
    if (!match) {
        console.error('Could not find version in Cargo.toml');
        process.exit(1);
    }
    return match[1];
}

function replaceInFile(filePath, searchRegex, replacement) {
    const fullPath = path.join(ROOT, filePath);
    if (!fs.existsSync(fullPath)) {
        console.warn(`Skipping ${filePath} (not found)`);
        return;
    }
    let content = fs.readFileSync(fullPath, 'utf8');
    const newContent = content.replace(searchRegex, replacement);
    if (content === newContent) {
        console.log(`  ${filePath} — already up to date`);
    } else {
        fs.writeFileSync(fullPath, newContent, 'utf8');
        console.log(`  ${filePath} — updated`);
    }
}

function main() {
    const version = readCargoVersion();
    console.log(`Syncing version v${version} from Cargo.toml...\n`);

    // Tauri app config
    replaceInFile(
        'client-gui/src-tauri/tauri.conf.json',
        /"version":\s*"[^"]+"/,
        `"version": "${version}"`
    );

    // client-gui package.json
    replaceInFile(
        'client-gui/package.json',
        /"version":\s*"[^"]+"/,
        `"version": "${version}"`
    );

    // Vue app constant
    replaceInFile(
        'client-gui/src/App.vue',
        /const APP_VERSION = 'v[^']+';/,
        `const APP_VERSION = 'v${version}';`
    );

    // Web package.json
    replaceInFile(
        'web/package.json',
        /"version":\s*"[^"]+"/,
        `"version": "${version}"`
    );

    // Web UI version badge
    replaceInFile(
        'web/src/views/ControlView.vue',
        /<span class="version-tag">v[^<]+<\/span>/,
        `<span class="version-tag">v${version}</span>`
    );

    console.log(`\nAll files synced to v${version}.`);
    console.log('Remember to run npm install in client-gui/ and web/ to update package-lock.json files.');
}

main();
