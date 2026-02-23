/**
 * Renderer-side client for AI-generated AWS CLI examples.
 * All requests go via IPC to the main process; the API key is never exposed.
 */

export interface GenerateCliResponse {
  command: string;
  explanation: string;
}

/**
 * Request an AWS CLI example from the AI service (main process calls REST API).
 */
export async function generateAwsCliExample(prompt: string): Promise<GenerateCliResponse> {
  if (!window.electron?.generateAwsCli) {
    return {
      command: '',
      explanation: 'Terminal AI is not available (missing IPC).',
    };
  }
  return window.electron.generateAwsCli({ prompt });
}
