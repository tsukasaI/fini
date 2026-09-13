import type { ChildProcessWithoutNullStreams } from 'child_process';

/**
 * Collects a child process's stdout/stderr as UTF-8 strings.
 *
 * `setEncoding('utf8')` makes each stream use Node's internal StringDecoder,
 * which buffers a trailing partial multi-byte sequence until the next chunk
 * arrives instead of decoding each chunk in isolation. Without it, a
 * multi-byte character split across a chunk boundary (pipes deliver ~64 KiB
 * chunks, not aligned to UTF-8 character boundaries) gets each half decoded
 * independently and replaced with U+FFFD.
 */
export function collectOutput(proc: ChildProcessWithoutNullStreams): {
    stdout: () => string;
    stderr: () => string;
} {
    let stdout = '';
    let stderr = '';

    proc.stdout.setEncoding('utf8');
    proc.stdout.on('data', (chunk: string) => {
        stdout += chunk;
    });

    proc.stderr.setEncoding('utf8');
    proc.stderr.on('data', (chunk: string) => {
        stderr += chunk;
    });

    return {
        stdout: () => stdout,
        stderr: () => stderr,
    };
}
