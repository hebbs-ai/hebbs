#!/usr/bin/env npx tsx
import { createInterface } from 'node:readline';
import { existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Command } from 'commander';
import chalk from 'chalk';
import { HebbsClient } from '@hebbs/sdk';
import { defaultConfig, loadConfig, type DemoConfig } from './config.js';
import { DisplayManager, type Verbosity } from './display.js';
import { SalesAgent } from './agent.js';
import { ALL_SCENARIOS, type ScenarioResult } from './scenarios/index.js';

const __dirname = dirname(fileURLToPath(import.meta.url));

function resolveConfig(configPath?: string): DemoConfig {
  if (configPath) {
    if (existsSync(configPath)) return loadConfig(configPath);
    const candidate = join(__dirname, '..', 'configs', `${configPath}.toml`);
    if (existsSync(candidate)) return loadConfig(candidate);
    return loadConfig(configPath);
  }
  const defaultPath = join(__dirname, '..', 'configs', 'openai.toml');
  if (existsSync(defaultPath)) return loadConfig(defaultPath);
  return defaultConfig();
}

const BANNER = `
${chalk.bold.cyan('   +===========================================================+')}
${chalk.bold.cyan('   |          HEBBS Demo -- Meet "Atlas"                        |')}
${chalk.bold.cyan('   |          Your AI Sales Agent for HEBBS (TypeScript)        |')}
${chalk.bold.cyan('   +===========================================================+')}

${chalk.dim('Atlas sells HEBBS -- a cognitive memory engine for AI applications.')}
${chalk.dim('You are the prospect. Every message is embedded, recalled, and')}
${chalk.dim('remembered -- watch the engine work in real time.')}
`;

const HELP = `
${chalk.bold('Conversation')}
  Just type naturally -- you are the prospect, Atlas responds.

${chalk.bold('Inspect HEBBS Brain')}
  ${chalk.cyan('/recall')} <query>    Manually query HEBBS recall
  ${chalk.cyan('/count')}             Total memory count

${chalk.bold('Engine Operations')}
  ${chalk.cyan('/reflect')}           Trigger HEBBS reflect
  ${chalk.cyan('/forget')} [entity]   Forget all memories for an entity
  ${chalk.cyan('/insights')}          Show accumulated insights

${chalk.bold('Session')}
  ${chalk.cyan('/help')}              Show this help
  ${chalk.cyan('quit')}               Exit
`;

async function interactive(configPath: string | undefined, verbosity: Verbosity, mockLlm: boolean, entity: string) {
  const cfg = resolveConfig(configPath);
  const display = new DisplayManager(verbosity);

  console.log(BANNER);

  let hebbs: HebbsClient;
  try {
    hebbs = new HebbsClient(cfg.hebbs.address);
    await hebbs.connect();
  } catch (e) {
    console.log(chalk.red(`Failed to connect to HEBBS server at ${cfg.hebbs.address}: ${e}`));
    process.exit(1);
  }

  const agent = new SalesAgent(cfg, hebbs, display, mockLlm);
  const llmLabel = mockLlm ? chalk.yellow('mock') : chalk.green(`openai/${cfg.llm.conversationModel}`);
  console.log(`  LLM:       ${llmLabel}`);
  console.log(`  Server:    ${chalk.green(cfg.hebbs.address)}`);
  console.log(`  Entity:    ${chalk.bold(entity)}`);
  console.log(`  Verbosity: ${verbosity}`);
  console.log();

  await agent.startSession(entity, 1);
  console.log(chalk.dim('Type /help for commands.\n'));

  const rl = createInterface({ input: process.stdin, output: process.stdout });
  const prompt = (): Promise<string> =>
    new Promise((resolve) => rl.question(chalk.bold.blue('You: '), resolve));

  try {
    while (true) {
      let input: string;
      try {
        input = (await prompt()).trim();
      } catch {
        console.log(chalk.dim('\nGoodbye!'));
        break;
      }
      if (!input) continue;
      if (['quit', 'exit', 'q'].includes(input.toLowerCase())) {
        console.log(chalk.dim('Goodbye!'));
        break;
      }
      if (input.startsWith('/')) {
        await handleCommand(input, agent, hebbs, entity);
        continue;
      }
      await agent.processTurn(input, ['similarity', 'temporal', 'causal', 'analogical']);
    }
  } finally {
    await agent.endSession();
    rl.close();
    await hebbs.close();
  }
}

async function handleCommand(cmd: string, agent: SalesAgent, hebbs: HebbsClient, entity: string) {
  const [command, ...rest] = cmd.trim().split(/\s+/);
  const arg = rest.join(' ');

  switch (command.toLowerCase()) {
    case '/recall':
      if (!arg) { console.log(chalk.dim('Usage: /recall <query>')); return; }
      try {
        const out = await hebbs.recall({ cue: arg, strategies: ['similarity', 'temporal'], topK: 5, entityId: entity });
        for (const r of out.results) {
          console.log(`  [${r.score.toFixed(3)}] ${r.memory.content}`);
        }
      } catch (e) { console.log(chalk.red(`Error: ${e}`)); }
      break;
    case '/reflect':
      await agent.runReflect(entity);
      break;
    case '/forget': {
      const target = arg || entity;
      await agent.runForget(target);
      break;
    }
    case '/insights':
      try {
        const ins = await hebbs.insights({ entityId: entity, maxResults: 10 });
        if (ins.length) {
          for (let i = 0; i < ins.length; i++) {
            console.log(`  ${i + 1}. [${ins[i].importance.toFixed(2)}] ${ins[i].content}`);
          }
        } else {
          console.log(chalk.dim('No insights yet. Run /reflect first.'));
        }
      } catch (e) { console.log(chalk.red(`Error: ${e}`)); }
      break;
    case '/count':
      try {
        const c = await hebbs.count();
        console.log(`${chalk.bold('Total memories:')} ${c}`);
      } catch (e) { console.log(chalk.red(`Error: ${e}`)); }
      break;
    case '/help':
      console.log(HELP);
      break;
    default:
      console.log(chalk.dim(`Unknown command: ${command}. Type /help for commands.`));
  }
}

async function runScenarios(configPath: string | undefined, verbosity: Verbosity, runAll: boolean, scenarioName: string | undefined, mockLlm: boolean) {
  const cfg = resolveConfig(configPath);

  let names: string[];
  if (scenarioName) {
    names = [scenarioName];
  } else if (runAll) {
    names = Object.keys(ALL_SCENARIOS);
  } else {
    console.log(chalk.yellow('Specify --all or --run <name>'));
    console.log(`Available: ${Object.keys(ALL_SCENARIOS).join(', ')}`);
    return;
  }

  const results: ScenarioResult[] = [];
  for (const name of names) {
    const Cls = ALL_SCENARIOS[name];
    if (!Cls) {
      console.log(chalk.red(`Unknown scenario: ${name}`));
      continue;
    }

    console.log(`\n${chalk.bold('Running scenario:')} ${name}`);
    const scenario = new Cls(cfg, verbosity, mockLlm);
    const result = await scenario.run();
    results.push(result);

    const status = result.passed ? chalk.green('PASS') : chalk.red('FAIL');
    console.log(`  ${status} (${result.elapsedMs.toFixed(0)}ms, ${result.assertions.length} assertions)`);

    if (result.error) console.log(chalk.red(`  Error: ${result.error}`));
    for (const a of result.assertions.filter((a) => !a.passed)) {
      console.log(chalk.red(`  FAIL: ${a.name}: ${a.message}`));
    }
  }

  console.log();
  const passed = results.filter((r) => r.passed).length;
  const total = results.length;
  const totalMs = results.reduce((s, r) => s + r.elapsedMs, 0);
  console.log(`${chalk.bold(`${passed}/${total} scenarios passed`)} (${(totalMs / 1000).toFixed(1)}s)`);
}

const program = new Command();
program.name('hebbs-demo').version('0.3.0').description('HEBBS Demo: AI Sales Intelligence Agent (TypeScript, gRPC)');

program
  .command('interactive')
  .description('Start an interactive conversation with the AI sales agent')
  .option('--config <path>', 'Config name or path to TOML file')
  .option('--verbosity <level>', 'Display verbosity (quiet/normal/verbose)', 'verbose')
  .option('--mock-llm', 'Use mock LLM (no API keys needed)', false)
  .option('--entity <id>', 'Entity ID for the conversation', 'prospect')
  .action((opts) => {
    interactive(opts.config, opts.verbosity as Verbosity, opts.mockLlm, opts.entity).catch(console.error);
  });

program
  .command('scenarios')
  .description('Run scripted scenario tests')
  .option('--config <path>', 'Config name or path to TOML file')
  .option('--verbosity <level>', 'Display verbosity', 'normal')
  .option('--all', 'Run all scenarios', false)
  .option('--run <name>', 'Run a specific scenario')
  .option('--mock-llm', 'Use mock LLM', true)
  .option('--real-llm', 'Use real LLM')
  .action((opts) => {
    const mockLlm = opts.realLlm ? false : opts.mockLlm;
    runScenarios(opts.config, opts.verbosity as Verbosity, opts.all, opts.run, mockLlm).catch(console.error);
  });

program.parse();
