export interface InterMcpClientOptions {
  plugin?: string | null;
}

export interface ToolDefinition {
  name: string;
  description: string;
  inputSchema: Record<string, any>;
}

export interface ResourceDefinition {
  uri: string;
  name: string;
  description?: string;
  mimeType?: string;
}

export interface PromptDefinition {
  name: string;
  description: string;
  arguments?: Array<{ name: string; description?: string; required: boolean }>;
}

export interface CallToolResult {
  content: Array<{ type: string; text?: string; data?: string }>;
  isError: boolean;
}

export class InterMcpClient {
  constructor(options?: InterMcpClientOptions);
  start(): Promise<void>;
  request(method: string, params?: Record<string, any>): Promise<any>;
  listTools(): Promise<ToolDefinition[]>;
  callTool(name: string, args?: Record<string, any>): Promise<CallToolResult>;
  listResources(): Promise<ResourceDefinition[]>;
  readResource(uri: string): Promise<any>;
  listPrompts(): Promise<PromptDefinition[]>;
  getPrompt(name: string, args?: Record<string, any>): Promise<any>;
  stop(): void;
}
