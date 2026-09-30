import { afterAll } from 'bun:test';
import { CAPTURE_MS } from './helpers/eval-budgets';
import { describeIfSelected, testIfSelected, createEvalCollector, finalizeEvalCollector } from './helpers/e2e-helpers';
import { QA_FUNCTIONAL_CASES, runQAFunctionalCase } from './helpers/qa-functional-eval';

const collector = createEvalCollector('e2e-qa-functional-fix');
describeIfSelected('Functional QA fix', ['qa-functional-cli-fix', 'qa-functional-webhook-fix'], () => {
  testIfSelected('qa-functional-cli-fix', () => runQAFunctionalCase(QA_FUNCTIONAL_CASES[2], collector), CAPTURE_MS);
  testIfSelected('qa-functional-webhook-fix', () => runQAFunctionalCase(QA_FUNCTIONAL_CASES[3], collector), CAPTURE_MS);
});
afterAll(async () => { await finalizeEvalCollector(collector); });
