/**
 * Mock AWS CLI command tree for the Command Explorer.
 * Covers common services so search finds useful results.
 * Data lives in awsclimockdata.json; merge with awsclidata.json (scraped) when loading the full tree.
 * @see https://docs.aws.amazon.com/cli/latest/
 */

import awsclimockdata from './awsclimockdata.json';

export interface AwsCliOption {
  name: string;
  short?: string;
  description: string;
  required?: boolean;
}

export interface AwsCliExample {
  description: string;
  command: string;
}

export interface AwsCliCommand {
  id: string;
  name: string;
  description: string;
  syntax: string;
  options: AwsCliOption[];
  examples: AwsCliExample[];
  /** True if from hand-curated mock data, false if from scraped docs. */
  mocked?: boolean;
  /** URL to AWS CLI reference doc for this command (scraped commands; mock can omit). */
  docUrl?: string;
  children?: AwsCliCommand[];
}

/** Mock tree (hand-curated). Merge with awsclidata.json in app if desired. */
export const AWS_CLI_MOCK_TREE: AwsCliCommand[] = awsclimockdata as AwsCliCommand[];
