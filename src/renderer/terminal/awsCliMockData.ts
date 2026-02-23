/**
 * Mock AWS CLI command tree for the Command Explorer.
 * Covers common services so search finds useful results.
 * @see https://docs.aws.amazon.com/cli/latest/
 */

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
  children?: AwsCliCommand[];
}

function opt(name: string, desc: string, required = false): AwsCliOption {
  return { name, description: desc, required };
}

function ex(desc: string, command: string): AwsCliExample {
  return { description: desc, command };
}

/** Build a leaf command (no children). */
function cmd(
  id: string,
  name: string,
  description: string,
  syntax: string,
  options: AwsCliOption[],
  examples: AwsCliExample[]
): AwsCliCommand {
  return { id, name, description, syntax, options, examples };
}

export const AWS_CLI_MOCK_TREE: AwsCliCommand[] = [
  {
    id: 's3',
    name: 's3',
    description: 'Amazon Simple Storage Service - object storage, buckets, upload/download.',
    syntax: 'aws s3 <command> [options]',
    options: [],
    examples: [],
    children: [
      cmd('s3-ls', 'ls', 'List S3 objects and prefixes.', 'aws s3 ls <s3://bucket/prefix> [options]', [opt('--profile', 'Use a specific profile'), opt('--recursive', 'List recursively')], [ex('List bucket root', 'aws s3 ls s3://my-bucket/'), ex('With profile', 'aws s3 ls s3://my-bucket/ --profile myprofile')]),
      cmd('s3-cp', 'cp', 'Copy files to/from S3.', 'aws s3 cp <source> <destination> [options]', [opt('--recursive', 'Copy directory recursively')], [ex('Upload file', 'aws s3 cp ./local.txt s3://my-bucket/path/'), ex('Download file', 'aws s3 cp s3://my-bucket/file.txt ./')]),
      cmd('s3-mb', 'mb', 'Create a bucket.', 'aws s3 mb s3://bucket-name [options]', [opt('--region', 'Region for the bucket')], [ex('Create bucket', 'aws s3 mb s3://my-new-bucket')]),
      cmd('s3-rb', 'rb', 'Remove a bucket.', 'aws s3 rb s3://bucket-name [options]', [opt('--force', 'Remove non-empty bucket')], [ex('Remove bucket', 'aws s3 rb s3://my-bucket --force')]),
    ],
  },
  {
    id: 'sts',
    name: 'sts',
    description: 'Security Token Service - temporary credentials, assume role, caller identity.',
    syntax: 'aws sts <command> [options]',
    options: [],
    examples: [],
    children: [
      cmd('sts-get-caller-identity', 'get-caller-identity', 'Get current IAM identity (account, user/role).', 'aws sts get-caller-identity [options]', [opt('--profile', 'Use a specific profile')], [ex('Current identity', 'aws sts get-caller-identity'), ex('With profile', 'aws sts get-caller-identity --profile myprofile')]),
      cmd('sts-assume-role', 'assume-role', 'Assume an IAM role and get temporary credentials.', 'aws sts assume-role --role-arn <arn> --role-session-name <name>', [opt('--role-arn', 'ARN of the role'), opt('--role-session-name', 'Session name')], [ex('Assume role', 'aws sts assume-role --role-arn arn:aws:iam::123456789012:role/MyRole --role-session-name mysession')]),
    ],
  },
  {
    id: 'ec2',
    name: 'ec2',
    description: 'Elastic Compute Cloud - instances, AMIs, security groups, VPCs.',
    syntax: 'aws ec2 <command> [options]',
    options: [],
    examples: [],
    children: [
      cmd('ec2-describe-instances', 'describe-instances', 'Describe EC2 instances.', 'aws ec2 describe-instances [options]', [opt('--instance-ids', 'Filter by instance IDs'), opt('--profile', 'Use a specific profile')], [ex('List all instances', 'aws ec2 describe-instances'), ex('By instance ID', 'aws ec2 describe-instances --instance-ids i-1234567890abcdef0')]),
      cmd('ec2-describe-images', 'describe-images', 'Describe AMIs (machine images).', 'aws ec2 describe-images [options]', [opt('--owners', 'Filter by owner (e.g. self, amazon)')], [ex('List my AMIs', 'aws ec2 describe-images --owners self')]),
      cmd('ec2-describe-security-groups', 'describe-security-groups', 'Describe security groups.', 'aws ec2 describe-security-groups [options]', [], [ex('List security groups', 'aws ec2 describe-security-groups')]),
    ],
  },
  {
    id: 'lambda',
    name: 'lambda',
    description: 'AWS Lambda - serverless functions, invoke, list functions.',
    syntax: 'aws lambda <command> [options]',
    options: [],
    examples: [],
    children: [
      cmd('lambda-list-functions', 'list-functions', 'List Lambda functions.', 'aws lambda list-functions [options]', [opt('--max-items', 'Max number to return'), opt('--profile', 'Use a specific profile')], [ex('List functions', 'aws lambda list-functions')]),
      cmd('lambda-invoke', 'invoke', 'Invoke a Lambda function.', 'aws lambda invoke --function-name <name> [options] outfile', [opt('--function-name', 'Function name or ARN'), opt('--payload', 'JSON payload')], [ex('Invoke function', 'aws lambda invoke --function-name myFunc output.json')]),
      cmd('lambda-get-function', 'get-function', 'Get function configuration and code location.', 'aws lambda get-function --function-name <name>', [opt('--function-name', 'Function name or ARN')], [ex('Get function', 'aws lambda get-function --function-name myFunc')]),
    ],
  },
  {
    id: 'dynamodb',
    name: 'dynamodb',
    description: 'DynamoDB - NoSQL database, tables, scan, query.',
    syntax: 'aws dynamodb <command> [options]',
    options: [],
    examples: [],
    children: [
      cmd('dynamodb-list-tables', 'list-tables', 'List DynamoDB tables.', 'aws dynamodb list-tables [options]', [opt('--limit', 'Max tables to return')], [ex('List tables', 'aws dynamodb list-tables')]),
      cmd('dynamodb-scan', 'scan', 'Scan a table.', 'aws dynamodb scan --table-name <name> [options]', [opt('--table-name', 'Table name')], [ex('Scan table', 'aws dynamodb scan --table-name MyTable')]),
      cmd('dynamodb-describe-table', 'describe-table', 'Describe a table.', 'aws dynamodb describe-table --table-name <name>', [opt('--table-name', 'Table name')], [ex('Describe table', 'aws dynamodb describe-table --table-name MyTable')]),
    ],
  },
  {
    id: 'iam',
    name: 'iam',
    description: 'Identity and Access Management - users, roles, policies.',
    syntax: 'aws iam <command> [options]',
    options: [],
    examples: [],
    children: [
      cmd('iam-list-users', 'list-users', 'List IAM users.', 'aws iam list-users [options]', [], [ex('List users', 'aws iam list-users')]),
      cmd('iam-list-roles', 'list-roles', 'List IAM roles.', 'aws iam list-roles [options]', [], [ex('List roles', 'aws iam list-roles')]),
      cmd('iam-get-user', 'get-user', 'Get IAM user details.', 'aws iam get-user [--user-name <name>]', [opt('--user-name', 'User name (default: current)')], [ex('Get current user', 'aws iam get-user')]),
    ],
  },
  {
    id: 'cloudformation',
    name: 'cloudformation',
    description: 'CloudFormation - infrastructure as code, stacks, templates.',
    syntax: 'aws cloudformation <command> [options]',
    options: [],
    examples: [],
    children: [
      cmd('cfn-list-stacks', 'list-stacks', 'List CloudFormation stacks.', 'aws cloudformation list-stacks [options]', [opt('--stack-status-filter', 'Filter by status')], [ex('List stacks', 'aws cloudformation list-stacks')]),
      cmd('cfn-describe-stacks', 'describe-stacks', 'Describe stacks.', 'aws cloudformation describe-stacks [options]', [opt('--stack-name', 'Stack name')], [ex('Describe stack', 'aws cloudformation describe-stacks --stack-name myStack')]),
      cmd('cfn-create-stack', 'create-stack', 'Create a stack from a template.', 'aws cloudformation create-stack --stack-name <name> --template-body <body>', [opt('--stack-name', 'Stack name'), opt('--template-body', 'Template JSON/YAML')], [ex('Create stack', 'aws cloudformation create-stack --stack-name myStack --template-body file://template.yaml')]),
    ],
  },
  {
    id: 'rds',
    name: 'rds',
    description: 'Relational Database Service - databases, instances, snapshots.',
    syntax: 'aws rds <command> [options]',
    options: [],
    examples: [],
    children: [
      cmd('rds-describe-db-instances', 'describe-db-instances', 'Describe RDS instances.', 'aws rds describe-db-instances [options]', [opt('--db-instance-identifier', 'Instance ID')], [ex('List DB instances', 'aws rds describe-db-instances')]),
      cmd('rds-describe-db-snapshots', 'describe-db-snapshots', 'Describe DB snapshots.', 'aws rds describe-db-snapshots [options]', [], [ex('List snapshots', 'aws rds describe-db-snapshots')]),
    ],
  },
  {
    id: 'sns',
    name: 'sns',
    description: 'Simple Notification Service - topics, publish, subscribe.',
    syntax: 'aws sns <command> [options]',
    options: [],
    examples: [],
    children: [
      cmd('sns-list-topics', 'list-topics', 'List SNS topics.', 'aws sns list-topics [options]', [], [ex('List topics', 'aws sns list-topics')]),
      cmd('sns-publish', 'publish', 'Publish a message to a topic.', 'aws sns publish --topic-arn <arn> --message <msg>', [opt('--topic-arn', 'Topic ARN'), opt('--message', 'Message text')], [ex('Publish message', 'aws sns publish --topic-arn arn:aws:sns:us-east-1:123456789012:MyTopic --message "Hello"')]),
    ],
  },
  {
    id: 'sqs',
    name: 'sqs',
    description: 'Simple Queue Service - message queues, send, receive.',
    syntax: 'aws sqs <command> [options]',
    options: [],
    examples: [],
    children: [
      cmd('sqs-list-queues', 'list-queues', 'List SQS queues.', 'aws sqs list-queues [options]', [], [ex('List queues', 'aws sqs list-queues')]),
      cmd('sqs-send-message', 'send-message', 'Send a message to a queue.', 'aws sqs send-message --queue-url <url> --message-body <body>', [opt('--queue-url', 'Queue URL'), opt('--message-body', 'Message body')], [ex('Send message', 'aws sqs send-message --queue-url https://sqs.us-east-1.amazonaws.com/123456789012/my-queue --message-body "Hello"')]),
    ],
  },
  {
    id: 'ecr',
    name: 'ecr',
    description: 'Elastic Container Registry - Docker images.',
    syntax: 'aws ecr <command> [options]',
    options: [],
    examples: [],
    children: [
      cmd('ecr-describe-repositories', 'describe-repositories', 'List ECR repositories.', 'aws ecr describe-repositories [options]', [], [ex('List repositories', 'aws ecr describe-repositories')]),
      cmd('ecr-get-login-password', 'get-login-password', 'Get password for docker login.', 'aws ecr get-login-password [--region <region>]', [opt('--region', 'Region')], [ex('Get login', 'aws ecr get-login-password --region us-east-1')]),
    ],
  },
  {
    id: 'logs',
    name: 'logs',
    description: 'CloudWatch Logs - log groups, streams, filter events.',
    syntax: 'aws logs <command> [options]',
    options: [],
    examples: [],
    children: [
      cmd('logs-describe-log-groups', 'describe-log-groups', 'List log groups.', 'aws logs describe-log-groups [options]', [], [ex('List log groups', 'aws logs describe-log-groups')]),
      cmd('logs-filter-log-events', 'filter-log-events', 'Filter log events.', 'aws logs filter-log-events --log-group-name <name> [options]', [opt('--log-group-name', 'Log group name'), opt('--filter-pattern', 'Filter pattern')], [ex('Filter logs', 'aws logs filter-log-events --log-group-name /aws/lambda/myFunc')]),
    ],
  },
  {
    id: 'secretsmanager',
    name: 'secrets-manager',
    description: 'Secrets Manager - store and retrieve secrets.',
    syntax: 'aws secretsmanager <command> [options]',
    options: [],
    examples: [],
    children: [
      cmd('sm-list-secrets', 'list-secrets', 'List secrets.', 'aws secretsmanager list-secrets [options]', [], [ex('List secrets', 'aws secretsmanager list-secrets')]),
      cmd('sm-get-secret-value', 'get-secret-value', 'Get secret value.', 'aws secretsmanager get-secret-value --secret-id <id>', [opt('--secret-id', 'Secret ID or ARN')], [ex('Get secret', 'aws secretsmanager get-secret-value --secret-id my-secret')]),
    ],
  },
];
