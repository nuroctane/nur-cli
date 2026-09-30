import { afterAll } from 'bun:test';
import { CAPTURE_MS } from './helpers/eval-budgets';
import { describeIfSelected, testIfSelected, createEvalCollector, finalizeEvalCollector } from './helpers/e2e-helpers';
import { QA_FUNCTIONAL_CASES, runQAFunctionalCase } from './helpers/qa-functional-eval';

const collector = createEvalCollector('e2e-qa-functional');
describeIfSelected('Functional QA report-only', ['qa-functional-cli-report', 'qa-functional-webhook-report'], () => {
  testIfSelected('qa-functional-cli-report', () => runQAFunctionalCase(QA_FUNCTIONAL_CASES[0], collector), CAPTURE_MS);
  testIfSelected('qa-functional-webhook-report', () => runQAFunctionalCase(QA_FUNCTIONAL_CASES[1], collector), CAPTURE_MS);
});
afterAll(async () => { await finalizeEvalCollector(collector); });
