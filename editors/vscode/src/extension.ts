import * as vscode from 'vscode';
import { spawn } from 'child_process';

let outputChannel: vscode.OutputChannel;

export function activate(context: vscode.ExtensionContext) {
    outputChannel = vscode.window.createOutputChannel('fini');

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
