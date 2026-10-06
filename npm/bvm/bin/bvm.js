#!/usr/bin/env node
// Runs the bvm binary for this platform, installed by npm as one of the
// optional @absolutejs/bvm-<platform> packages. Each binary is the one listed
// in that release's signed SHASUMS256.txt.
'use strict';
const { spawnSync } = require('node:child_process');
const path = require('node:path');

const platform = { darwin: 'darwin', linux: 'linux', win32: 'windows' }[process.platform];
const arch = { arm64: 'arm64', x64: 'x64' }[process.arch];
if (!platform || !arch) {
	console.error(`bvm: no build for ${process.platform}-${process.arch}`);
	process.exit(1);
}
const pkg = `@absolutejs/bvm-${platform}-${arch}`;
let binary;
try {
	binary = path.join(
		path.dirname(require.resolve(`${pkg}/package.json`)),
		'bin',
		platform === 'windows' ? 'bvm.exe' : 'bvm'
	);
} catch {
	console.error(`bvm: ${pkg} is not installed (was the install run with --no-optional?)`);
	process.exit(1);
}
const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' });
if (result.error) {
	console.error(`bvm: ${result.error.message}`);
	process.exit(1);
}
process.exit(result.status ?? 1);
