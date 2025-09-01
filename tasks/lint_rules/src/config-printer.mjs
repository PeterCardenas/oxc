import { stdout } from 'node:process';
import { ALL_TARGET_PLUGINS, createESLintLinter, loadTargetPluginRules } from './eslint-rules.mjs';
import { parseArgs } from 'node:util';

const HELP = `
Usage:
  $ cmd --plugin=<pluginName> --rule=<ruleName> [--help]

Options:
  --plugin, -p: Plugin name
  --rule, -r: Rule name
  --help, -h: Print this help message

Plugins: ${Array.from(ALL_TARGET_PLUGINS.keys()).join(', ')}
`;

(async () => {
  const { values } = parseArgs({
    options: {
      plugin: { type: 'string', short: 'p' },
      rule: { type: 'string', short: 'r' },
      help: { type: 'boolean', short: 'h' },
    },
  });

  if (values.help || !values.plugin || !values.rule) {
    console.log(HELP);
    process.exit(1);
  }

  const linter = createESLintLinter();
  loadTargetPluginRules(linter);
  const ruleName = values.plugin === 'eslint' ? values.rule : `${values.plugin}/${values.rule}`;
  const rule = linter.getRules().get(ruleName);
  if (!rule?.meta?.schema) {
    console.error(`No schema for rule ${ruleName}`);
    process.exit(1);
  }
  stdout.write("module.exports = ");
  console.dir({ meta: { schema: rule.meta.schema } }, { depth: Infinity });
})();
