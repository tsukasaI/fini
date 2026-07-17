import * as vscode from 'vscode';
import { spawn } from 'child_process';

let outputChannel: vscode.OutputChannel;

// CLI versions this extension is tested against. The extension and the CLI
// version independently (Marketplace auto-update vs brew/cargo), so an
// untested pairing is warned about once on activation — formatting is never
// blocked. Widen/shift this range in the release that adapts to a --stdin
// contract change (see release-playbook).
const COMPATIBLE_CLI_MIN: [number, number, number] = [0, 3, 0];
const COMPATIBLE_CLI_MAX_EXCLUSIVE: [number, number, number] = [0, 5, 0];

export function activate(context: vscode.ExtensionContext) {
    outputChannel = vscode.window.createOutputChannel('fini');

    const config = vscode.workspace.getConfiguration('fini');
    if (config.get<boolean>('enable')) {
        checkCliCompatibility(config.get<string>('path') || 'fini');
    }

    const onSaveDisposable = vscode.workspace.onWillSaveTextDocument((event) => {
        const config = vscode.workspace.getConfiguration('fini');
        if (!config.get<boolean>('enable') || !config.get<boolean>('formatOnSave')) {
            return;
        }

        const document = event.document;
        if (document.uri.scheme !== 'file') {
            return;
        }

        event.waitUntil(formatDocument(document));
    });

    const formatCommand = vscode.commands.registerCommand('fini.formatDocument', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            vscode.window.showWarningMessage('No active editor');
            return;
        }

        const edits = await formatDocument(editor.document);
        if (edits && edits.length > 0) {
            const edit = new vscode.WorkspaceEdit();
            edit.set(editor.document.uri, edits);
            await vscode.workspace.applyEdit(edit);
        }
    });

    context.subscriptions.push(onSaveDisposable, formatCommand, outputChannel);
}

function compareVersions(a: [number, number, number], b: [number, number, number]): number {
    for (let i = 0; i < 3; i++) {
        if (a[i] !== b[i]) {
            return a[i] - b[i];
        }
    }
    return 0;
}

function checkCliCompatibility(finiPath: string) {
    const proc = spawn(finiPath, ['--version']);

    let stdout = '';
    proc.stdout.on('data', (data) => {
        stdout += data.toString();
    });

    proc.on('close', (code) => {
        if (code !== 0) {
            return;
        }
        // `fini X.Y.Z` — shape covered by the CLI's version-output test
        const match = stdout.trim().match(/^fini (\d+)\.(\d+)\.(\d+)/);
        if (!match) {
            outputChannel.appendLine(
                `Unrecognized \`fini --version\` output: ${stdout.trim()}`
            );
            return;
        }
        const version: [number, number, number] = [
            Number(match[1]),
            Number(match[2]),
            Number(match[3]),
        ];
        if (
            compareVersions(version, COMPATIBLE_CLI_MIN) < 0 ||
            compareVersions(version, COMPATIBLE_CLI_MAX_EXCLUSIVE) >= 0
        ) {
            const range = `>=${COMPATIBLE_CLI_MIN.join('.')} <${COMPATIBLE_CLI_MAX_EXCLUSIVE.join('.')}`;
            vscode.window.showWarningMessage(
                `fini CLI ${version.join('.')} is outside the range this extension was tested with (${range}). ` +
                'Formatting still runs, but update the CLI or the extension if results look wrong.'
            );
        }
    });

    // A missing binary is reported when formatting is first attempted;
    // the activation probe stays silent.
    proc.on('error', () => {});
}

async function formatDocument(document: vscode.TextDocument): Promise<vscode.TextEdit[]> {
    const config = vscode.workspace.getConfiguration('fini');
    const finiPath = config.get<string>('path') || 'fini';
    const additionalArgs = config.get<string[]>('args') || [];

    const originalText = document.getText();

    return new Promise((resolve) => {
        const args = ['--stdin', ...additionalArgs];
        const proc = spawn(finiPath, args, {
            cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath,
        });

        let stdout = '';
        let stderr = '';

        proc.stdout.on('data', (data) => {
            stdout += data.toString();
        });

        proc.stderr.on('data', (data) => {
            stderr += data.toString();
        });

        proc.on('close', (code) => {
            if (code !== 0) {
                outputChannel.appendLine(`fini exited with code ${code}`);
                if (stderr) {
                    outputChannel.appendLine(stderr);
                }
                resolve([]);
                return;
            }

            if (stdout === originalText) {
                resolve([]);
                return;
            }

            const fullRange = new vscode.Range(
                document.positionAt(0),
                document.positionAt(originalText.length)
            );
            resolve([vscode.TextEdit.replace(fullRange, stdout)]);
        });

        proc.on('error', (err) => {
            outputChannel.appendLine(`Failed to run fini: ${err.message}`);
            vscode.window.showErrorMessage(
                'fini not found. Install it via: cargo install fini'
            );
            resolve([]);
        });

        proc.stdin.write(originalText);
        proc.stdin.end();
    });
}

export function deactivate() {}
