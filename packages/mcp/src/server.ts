import { McpServer } from '@modelcontextprotocol/server'
import * as z from 'zod/v4'
import { codexInvocationContext } from './context.js'
import type { RelayService } from './service.js'

function result(value: unknown) {
  return {
    content: [{ type: 'text' as const, text: JSON.stringify(value) }],
    structuredContent: { result: value },
  }
}

function failure(error: unknown) {
  return {
    isError: true,
    content: [
      {
        type: 'text' as const,
        text: error instanceof Error ? error.message : String(error),
      },
    ],
  }
}

export function createRelayMcpServer(service: RelayService): McpServer {
  const server = new McpServer(
    { name: 'relay', version: '0.0.0' },
    {
      instructions:
        'List agents before delegation. Give run_agent a bounded task. Use wait_agent before reviewing the result.',
    },
  )

  server.registerTool(
    'list_agents',
    { description: 'List enabled Relay agent profiles available to this Codex session.' },
    async (context) => {
      try {
        return result(await service.listAgents(codexInvocationContext(context)))
      } catch (error) {
        return failure(error)
      }
    },
  )

  server.registerTool(
    'run_agent',
    {
      description: 'Start one bounded task using an enabled Relay agent profile.',
      inputSchema: z.object({
        agent_id: z.string().min(1),
        task: z.string().min(1),
        access_mode: z.enum(['read_only', 'propose', 'write']).optional(),
        isolation: z.enum(['shared', 'worktree']).optional(),
      }),
    },
    async ({ agent_id, task, access_mode, isolation }, context) => {
      try {
        return result(
          await service.runAgent(codexInvocationContext(context), {
            agentId: agent_id,
            task,
            ...(access_mode ? { accessMode: access_mode } : {}),
            ...(isolation ? { isolation } : {}),
          }),
        )
      } catch (error) {
        return failure(error)
      }
    },
  )

  const workerSchema = z.object({ worker_session_id: z.string().min(1) })
  server.registerTool(
    'get_agent_status',
    { description: 'Read the current projected state of a Relay worker.', inputSchema: workerSchema },
    async ({ worker_session_id }) => {
      try {
        return result(await service.status(worker_session_id))
      } catch (error) {
        return failure(error)
      }
    },
  )
  server.registerTool(
    'wait_agent',
    { description: 'Wait for a Relay worker to reach a terminal state.', inputSchema: workerSchema },
    async ({ worker_session_id }) => {
      try {
        return result(await service.wait(worker_session_id))
      } catch (error) {
        return failure(error)
      }
    },
  )
  server.registerTool(
    'send_agent',
    {
      description: 'Send a follow-up message when the selected Runtime supports it.',
      inputSchema: workerSchema.extend({ message: z.string().min(1) }),
    },
    async ({ worker_session_id, message }) => {
      try {
        await service.send(worker_session_id, message)
        return result({ sent: true })
      } catch (error) {
        return failure(error)
      }
    },
  )
  server.registerTool(
    'cancel_agent',
    { description: 'Cancel an active Relay worker.', inputSchema: workerSchema },
    async ({ worker_session_id }) => {
      try {
        await service.cancel(worker_session_id)
        return result({ cancelled: true })
      } catch (error) {
        return failure(error)
      }
    },
  )
  server.registerTool(
    'accept_agent',
    {
      description:
        'Mark an awaiting Relay task complete after Codex has reviewed the worker result.',
      inputSchema: workerSchema,
    },
    async ({ worker_session_id }) => {
      try {
        return result(await service.accept(worker_session_id))
      } catch (error) {
        return failure(error)
      }
    },
  )
  server.registerTool(
    'sync_session',
    {
      description: 'Synchronize the current Codex session identity and exact display name.',
      inputSchema: z.object({ session_id: z.string().optional() }),
    },
    async ({ session_id }, context) => {
      try {
        const invocation = codexInvocationContext(context)
        if (session_id && session_id !== invocation.threadId) {
          throw new Error('Hook session identity does not match the Codex request identity')
        }
        return result(await service.syncSession(invocation))
      } catch (error) {
        return failure(error)
      }
    },
  )
  server.registerTool(
    'end_session',
    {
      description: 'Mark the current Codex session as ended and clear temporary policy.',
      inputSchema: z.object({ session_id: z.string().optional() }),
    },
    async ({ session_id }, context) => {
      try {
        const invocation = codexInvocationContext(context)
        if (session_id && session_id !== invocation.threadId) {
          throw new Error('Hook session identity does not match the Codex request identity')
        }
        await service.endSession(invocation)
        return result({ ended: true })
      } catch (error) {
        return failure(error)
      }
    },
  )

  return server
}
