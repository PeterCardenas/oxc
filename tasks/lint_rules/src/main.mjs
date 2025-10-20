import { parseArgs } from 'node:util';
import { ALL_TARGET_PLUGINS, createESLintLinter, loadTargetPluginRules } from './eslint-rules.mjs';
import {
  createRuleEntries,
  overrideTypeScriptPluginStatusWithEslintPluginStatus as syncTypeScriptPluginStatusWithEslintPluginStatus,
  syncUnicornPluginStatusWithEslintPluginStatus,
  syncVitestPluginStatusWithJestPluginStatus,
  updateImplementedStatus,
  updateNotSupportedStatus,
} from './oxlint-rules.mjs';
import { spawn } from 'node:child_process';
import { createWriteStream, mkdirSync } from 'node:fs';

const HELP = `
Usage:
  $ cmd [--target=<pluginName>]... [--update] [--help]

Options:
  --target, -t: Which plugin to target, multiple allowed
  --update: Update the issue instead of printing to stdout
  --help, -h: Print this help message

Plugins: ${Array.from(ALL_TARGET_PLUGINS.keys()).join(', ')}
`;

void (async () => {
  //
  // Parse arguments
  //
  const { values } = parseArgs({
    options: {
      // Mainly for debugging
      target: { type: 'string', short: 't', multiple: true },
      update: { type: 'boolean' },
      help: { type: 'boolean', short: 'h' },
    },
  });

  if (values.help) return console.log(HELP);

  const targetPluginNames = new Set(values.target ?? ALL_TARGET_PLUGINS.keys());
  for (const pluginName of targetPluginNames) {
    if (!ALL_TARGET_PLUGINS.has(pluginName)) {
      console.error(`Unknown plugin name: ${String(pluginName)}`);
      return;
    }
  }

  //
  // Load linter and all plugins
  //
  const linter = createESLintLinter();
  loadTargetPluginRules(linter);

  //
  // Generate entry and update status
  //
  const ruleEntries = createRuleEntries(linter.getRules());
  await updateImplementedStatus(ruleEntries);
  updateNotSupportedStatus(ruleEntries);
  await syncTypeScriptPluginStatusWithEslintPluginStatus(ruleEntries);
  await syncVitestPluginStatusWithJestPluginStatus(ruleEntries);
  syncUnicornPluginStatusWithEslintPluginStatus(ruleEntries);

  /** @type {Map<string, string[]>} */
  const failedRules = new Map();
  for (const [fullRuleName, rule] of ruleEntries) {
    if (rule.isNotSupported) continue;
    const [pluginName, ruleName] = fullRuleName.split('/', 2);

    const justCommand = `new-${pluginName === "eslint" ? "" : `${pluginName}-`}rule`;
    /** @type {string[]} */
    const logs = [];
    const success = await /** @type {Promise<boolean>} */(new Promise((resolve, reject) => {
      const proc = spawn('just', [justCommand, ruleName], {});
      if (!proc) {
        reject(new Error('process failed'));
      }
      proc.stdout?.on('data', (data) => {
        process.stdout.write(data.toString());
        logs.push(data.toString());
      });
      proc.stderr?.on('data', (data) => {
        process.stderr.write(data.toString())
        logs.push(data.toString());
      });
      proc.on('exit', (code) => {
        if (code !== 0) {
          resolve(false);
        } else {
          resolve(true);
        }
      });
    }));
    // if (failedRules.size > 0) break;
    const parentDir = `./logs/${success ? 'success' : 'failed'}`;
    mkdirSync(parentDir, { recursive: true });
    const logFilePath = `${parentDir}/${fullRuleName.replaceAll('/', '__')}_log.txt`;
    const logStream = createWriteStream(logFilePath, { flags: 'a' });
    logStream.write(`Failed to run ${fullRuleName}:\n`);
    for (const log of logs) {
      logStream.write(log + '\n');
    }
    logStream.end();
  }
  console.log(failedRules.size, 'rules failed to run');

  //
  // Render list and update if necessary
  //
  // const results = await Promise.allSettled(
  //   Array.from(targetPluginNames).map((pluginName) => {
  //     const pluginMeta = /** @type {import("./eslint-rules.mjs").TargetPluginMeta} */ (
  //       ALL_TARGET_PLUGINS.get(pluginName)
  //     );
  //     const content = renderMarkdown(pluginName, pluginMeta, ruleEntries);
  //
  //     if (!values.update) return Promise.resolve(content);
  //     // Requires `env.GITHUB_TOKEN`
  //     return updateGitHubIssue(pluginMeta, content);
  //   }),
  // );
  // for (const result of results) {
  //   if (result.status === 'fulfilled') console.log(result.value);
  //   if (result.status === 'rejected') console.error(result.reason);
  // }
})();
