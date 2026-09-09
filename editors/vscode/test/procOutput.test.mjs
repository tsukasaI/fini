// Regression test for issue #88: reading a child process's stdout via
// per-chunk `Buffer.toString()` corrupts multi-byte UTF-8 characters that
// straddle a chunk boundary (pipes deliver ~64 KiB chunks, and 65536 is not
// a multiple of 3, the byte width of "あ" and similar CJK characters).
//
// Uses Node's built-in test runner (`node --test`) so no extra devDependency
// is needed. Run with: node --test editors/vscode/test/procOutput.test.mjs

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { collectOutput } from '../src/procOutput.ts';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..', '..', '..');
const finiBin = process.env.FINI_BIN || path.join(repoRoot, 'target', 'release', 'fini');

// The buggy pattern this issue fixes: per-chunk toString() with no encoding
// set on the stream, so each Buffer chunk is decoded independently. Used
// only to prove the test harness can detect the corruption it's guarding
// against — production code goes through collectOutput() from procOutput.ts.
function collectOutputBuggy(proc) {
    let stdout = '';
    proc.stdout.on('data', (data) => {
        stdout += data.toString();
    });
    return { stdout: () => stdout };
}

function runFini(collector) {
    return new Promise((resolve, reject) => {
        const proc = spawn(finiBin, ['--stdin'], { cwd: repoRoot });
        const output = collector(proc);

        proc.on('close', (code) => {
            if (code !== 0) {
                reject(new Error(`fini exited with code ${code}`));
                return;
            }
            resolve(output.stdout());
        });
        proc.on('error', (err) => {
            reject(
                new Error(
                    `failed to spawn fini at ${finiBin}: ${err.message}. ` +
                        'Build it first (`cargo build --release`) or point FINI_BIN at a built binary.'
                )
            );
        });

        // stdout arrives from the OS pipe in ~64 KiB chunks regardless of
        // how stdin is written; fini reads all of stdin before it emits
        // anything, so a single write is enough to trigger the read-side
        // chunking that this test guards against.
        proc.stdin.end(Buffer.from('あ'.repeat(100_000), 'utf8')); // 300,000 bytes
    });
}

test('collectOutput (fixed) preserves multi-byte characters split across chunk boundaries', async () => {
    // fini normalizes EOF newlines, so stdin without a trailing "\n" comes
    // back with one appended.
    const expected = `${'あ'.repeat(100_000)}\n`;
    const stdout = await runFini(collectOutput);
    assert.equal(stdout, expected);
    assert.equal(Buffer.byteLength(stdout, 'utf8'), 300_001);
    assert.equal(stdout.includes('�'), false);
});

test('collectOutput (buggy per-chunk toString) demonstrates the corruption this fix prevents', async () => {
    const expected = `${'あ'.repeat(100_000)}\n`;
    const stdout = await runFini(collectOutputBuggy);
    assert.notEqual(stdout, expected);
    assert.ok(stdout.includes('�'), 'expected replacement characters from split multi-byte sequences');
});
