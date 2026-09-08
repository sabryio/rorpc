import { createFileRoute } from "@tanstack/react-router";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { isDefinedError, orpc, ORPCError } from "@/rpc/server-reference";

export const Route = createFileRoute("/error-test")({
  component: ErrorTestPage,
});

function ErrorTestPage() {
  return (
    <div className="min-h-screen bg-neutral-50">
      {/* Header */}
      <header className="border-b border-neutral-200 bg-white">
        <div className="max-w-4xl mx-auto px-6 py-6">
          <div className="flex items-baseline gap-3">
            <h1 className="text-2xl font-semibold tracking-tight text-neutral-900">
              Error Handling Test
            </h1>
            <span className="text-sm text-neutral-400 font-mono">
              StoreError with #[from]
            </span>
          </div>
          <p className="text-sm text-neutral-600 mt-2">
            Testing thiserror #[from] integration with frontend error handling
          </p>
        </div>
      </header>

      <div className="max-w-4xl mx-auto px-6 py-12">
        <div className="space-y-8">
          <RateLimitErrorTest />
          <StoreErrorTest />
          <NotFoundErrorTest />
          <CreatePlanetErrors />
        </div>
      </div>
    </div>
  );
}

function RateLimitErrorTest() {
  const mutation = useMutation(
    orpc.ping.mutationOptions({
      retry: false,
    })
  );

  return (
    <div className="bg-white border border-neutral-200 rounded-lg p-6">
      <div className="mb-4">
        <h2 className="text-lg font-semibold text-neutral-900 mb-2">
          Rate Limit Error Test (Structured Data)
        </h2>
        <p className="text-sm text-neutral-600">
          Error with structured data: <code className="text-xs bg-neutral-100 px-1.5 py-0.5 rounded font-mono">{'{ retryAfter: number }'}</code>
          {" "}— demonstrating complex error payloads
        </p>
      </div>

      <div className="flex items-center gap-3 mb-4">
        <button
          onClick={() => mutation.mutate(undefined)}
          disabled={mutation.isPending}
          className="px-4 py-2 bg-neutral-900 text-white text-sm font-medium rounded-md hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          {mutation.isPending ? "Testing..." : "Trigger Rate Limit"}
        </button>
        <span className="text-xs text-neutral-500">
          (Backend needs to be updated to return this error)
        </span>
      </div>

      {mutation.error && (
        <div className="p-4 bg-orange-50 border-2 border-orange-300 rounded-lg">
          {mutation.error instanceof ORPCError ? (
            <div className="space-y-3">
              <div className="flex items-start gap-3">
                <span className="text-2xl">⏱️</span>
                <div className="flex-1">
                  <p className="font-semibold text-orange-900 text-base">
                    {mutation.error.code === "RATE_LIMIT_EXCEEDED" ? "Rate Limited!" : mutation.error.code}
                  </p>
                  <p className="text-orange-700 mt-1">{mutation.error.message || String(mutation.error.data)}</p>
                </div>
              </div>
              
              {mutation.error.code === "RATE_LIMIT_EXCEEDED" && typeof mutation.error.data === 'object' && mutation.error.data !== null && 'retryAfter' in mutation.error.data && (
                <div className="bg-orange-100 rounded p-3 text-orange-800">
                  <p className="font-medium text-sm mb-1">Structured Error Data:</p>
                  <p className="text-sm">
                    Retry after: <span className="font-mono font-semibold">{mutation.error.data.retryAfter}</span> seconds
                  </p>
                </div>
              )}

              <div className="bg-neutral-900 rounded p-3 font-mono text-xs text-orange-400 overflow-x-auto">
                <div className="text-neutral-400 mb-2">Full Error Object:</div>
                <pre className="whitespace-pre-wrap break-words">{JSON.stringify(mutation.error, null, 2)}</pre>
              </div>

              <div className="text-xs text-orange-600 bg-orange-50 p-2 rounded border border-orange-200">
                ✓ Structured error data with TypeScript types
                <br />
                ✓ Frontend can access <code className="bg-orange-100 px-1 rounded">error.data.retryAfter</code>
                <br />
                ✓ Full type safety with Zod schema
              </div>
            </div>
          ) : (
            <div className="space-y-3">
              <p className="text-orange-800">Unexpected error type (not ORPCError)</p>
              <div className="bg-neutral-900 rounded p-3 font-mono text-xs text-orange-400 overflow-x-auto">
                <pre className="whitespace-pre-wrap break-words">{JSON.stringify(mutation.error, null, 2)}</pre>
              </div>
            </div>
          )}
        </div>
      )}

      {mutation.data && (
        <div className="p-4 bg-emerald-50 border border-emerald-200 rounded-lg">
          <p className="text-emerald-800 font-medium">
            ✓ Response: {mutation.data}
          </p>
        </div>
      )}

      {!mutation.error && !mutation.data && !mutation.isPending && (
        <div className="p-4 bg-neutral-100 border border-neutral-200 rounded-lg text-neutral-600 text-sm">
          Click "Trigger Rate Limit" to test (requires backend implementation)
        </div>
      )}
    </div>
  );
}

function StoreErrorTest() {
  const [id, setId] = useState<number>(999);

  const { data, isLoading, error, refetch } = useQuery(
    orpc.planet.find.queryOptions({
      input: { id },
      retry: false,
    })
  );

  return (
    <div className="bg-white border border-neutral-200 rounded-lg p-6">
      <div className="mb-4">
        <h2 className="text-lg font-semibold text-neutral-900 mb-2">
          Store Error Test (ID: 999)
        </h2>
        <p className="text-sm text-neutral-600">
          Planet ID 999 triggers a <code className="text-xs bg-neutral-100 px-1.5 py-0.5 rounded font-mono">StoreError</code> 
          {" "}which is converted to <code className="text-xs bg-neutral-100 px-1.5 py-0.5 rounded font-mono">RpcError</code> 
          {" "}via <code className="text-xs bg-neutral-100 px-1.5 py-0.5 rounded font-mono">#[from]</code>
        </p>
      </div>

      <div className="flex items-center gap-3 mb-4">
        <input
          type="number"
          value={id}
          onChange={(e) => setId(Number(e.target.value))}
          className="w-24 px-3 py-2 text-sm border border-neutral-300 rounded-md focus:outline-none focus:ring-2 focus:ring-neutral-900"
        />
        <button
          onClick={() => refetch()}
          disabled={isLoading}
          className="px-4 py-2 bg-neutral-900 text-white text-sm font-medium rounded-md hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          {isLoading ? "Loading..." : "Fetch Planet"}
        </button>
      </div>

      {error && (
        <div className="p-4 bg-red-50 border-2 border-red-300 rounded-lg">
          {error instanceof ORPCError ? (
            <div className="space-y-3">
              <div className="flex items-start gap-3">
                <span className="text-2xl">🔥</span>
                <div className="flex-1">
                  <p className="font-semibold text-red-900 text-base">
                    {error.code === "STORE_ERROR" ? "Store Error Caught!" : error.code}
                  </p>
                  <p className="text-red-700 mt-1">{error.message || String(error.data)}</p>
                </div>
              </div>
              
              <div className="bg-red-100 rounded p-3 font-mono text-xs text-red-800">
                <div className="grid grid-cols-[120px_1fr] gap-2">
                  <span className="text-red-600 font-semibold">Error Code:</span>
                  <span>{error.code}</span>
                  
                  <span className="text-red-600 font-semibold">Message:</span>
                  <span>{error.message || 'N/A'}</span>
                  
                  <span className="text-red-600 font-semibold">Data:</span>
                  <span>{typeof error.data === 'string' ? error.data : JSON.stringify(error.data)}</span>
                  
                  <span className="text-red-600 font-semibold">From Backend:</span>
                  <span>StoreError → RpcError (#[from])</span>
                </div>
              </div>

              <div className="bg-neutral-900 rounded p-3 font-mono text-xs text-green-400 overflow-x-auto">
                <div className="text-neutral-400 mb-2">Full Error Object:</div>
                <pre className="whitespace-pre-wrap break-words">{JSON.stringify(error, null, 2)}</pre>
              </div>

              <div className="text-xs text-red-600 bg-red-50 p-2 rounded border border-red-200">
                ✓ Type-safe error handling working
                <br />
                ✓ thiserror #[from] conversion successful
                <br />
                ✓ Frontend received {error.code} code
                <br />
                ✓ isDefinedError: {isDefinedError(error) ? 'true' : 'false'}
              </div>
            </div>
          ) : (
            <div className="space-y-3">
              <p className="text-red-800">Unexpected error type (not ORPCError)</p>
              <div className="bg-neutral-900 rounded p-3 font-mono text-xs text-red-400 overflow-x-auto">
                <pre className="whitespace-pre-wrap break-words">{JSON.stringify(error, null, 2)}</pre>
              </div>
            </div>
          )}
        </div>
      )}

      {data && (
        <div className="p-4 bg-emerald-50 border border-emerald-200 rounded-lg">
          <p className="text-emerald-800 font-medium">
            ✓ Found: {data.name} (#{data.id})
          </p>
          {data.description && (
            <p className="text-emerald-700 text-sm mt-1">{data.description}</p>
          )}
        </div>
      )}

      {!error && !data && !isLoading && (
        <div className="p-4 bg-neutral-100 border border-neutral-200 rounded-lg text-neutral-600 text-sm">
          Click "Fetch Planet" to test the error
        </div>
      )}
    </div>
  );
}

function NotFoundErrorTest() {
  const [id, setId] = useState<number>(9999);

  const { data, isLoading, error, refetch } = useQuery(
    orpc.planet.find.queryOptions({
      input: { id },
      retry: false,
    })
  );

  return (
    <div className="bg-white border border-neutral-200 rounded-lg p-6">
      <div className="mb-4">
        <h2 className="text-lg font-semibold text-neutral-900 mb-2">
          Not Found Error Test
        </h2>
        <p className="text-sm text-neutral-600">
          Standard NOT_FOUND error for comparison
        </p>
      </div>

      <div className="flex items-center gap-3 mb-4">
        <input
          type="number"
          value={id}
          onChange={(e) => setId(Number(e.target.value))}
          className="w-24 px-3 py-2 text-sm border border-neutral-300 rounded-md focus:outline-none focus:ring-2 focus:ring-neutral-900"
        />
        <button
          onClick={() => refetch()}
          disabled={isLoading}
          className="px-4 py-2 bg-neutral-900 text-white text-sm font-medium rounded-md hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          {isLoading ? "Loading..." : "Fetch Planet"}
        </button>
      </div>

      {error && (
        <div className="p-4 bg-amber-50 border-2 border-amber-300 rounded-lg">
          {error instanceof ORPCError ? (
            <div className="space-y-2">
              <div className="flex items-start gap-3">
                <span className="text-xl">⚠️</span>
                <div>
                  <p className="font-semibold text-amber-900">{error.code}</p>
                  <p className="text-amber-700 text-sm mt-1">{error.message || String(error.data)}</p>
                </div>
              </div>
              
              <div className="bg-neutral-900 rounded p-3 font-mono text-xs text-amber-400 overflow-x-auto">
                <pre className="whitespace-pre-wrap break-words">{JSON.stringify(error, null, 2)}</pre>
              </div>
            </div>
          ) : (
            <div className="space-y-2">
              <p className="text-amber-800">Unexpected error type</p>
              <div className="bg-neutral-900 rounded p-3 font-mono text-xs text-amber-400 overflow-x-auto">
                <pre className="whitespace-pre-wrap break-words">{JSON.stringify(error, null, 2)}</pre>
              </div>
            </div>
          )}
        </div>
      )}

      {data && (
        <div className="p-4 bg-emerald-50 border border-emerald-200 rounded-lg">
          <p className="text-emerald-800 font-medium">
            ✓ Found: {data.name} (#{data.id})
          </p>
        </div>
      )}
    </div>
  );
}

function CreatePlanetErrors() {
  const [name, setName] = useState("");

  const mutation = useMutation(
    orpc.planet.create.mutationOptions({
      retry: false,
    })
  );

  return (
    <div className="bg-white border border-neutral-200 rounded-lg p-6">
      <div className="mb-4">
        <h2 className="text-lg font-semibold text-neutral-900 mb-2">
          Validation Errors Test
        </h2>
        <p className="text-sm text-neutral-600">
          Test BAD_REQUEST (empty name) and INTERNAL_ERROR (name too long)
        </p>
      </div>

      <div className="space-y-3 mb-4">
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Planet name"
          className="w-full px-3 py-2 text-sm border border-neutral-300 rounded-md focus:outline-none focus:ring-2 focus:ring-neutral-900"
        />
        
        <div className="flex gap-2">
          <button
            onClick={() => mutation.mutate({ name, description: undefined })}
            disabled={mutation.isPending}
            className="px-4 py-2 bg-neutral-900 text-white text-sm font-medium rounded-md hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
          >
            Create Planet
          </button>
          
          <button
            onClick={() => setName("")}
            className="px-4 py-2 bg-neutral-200 text-neutral-700 text-sm font-medium rounded-md hover:bg-neutral-300 transition-colors"
          >
            Test Empty (BAD_REQUEST)
          </button>
          
          <button
            onClick={() => setName("A".repeat(150))}
            className="px-4 py-2 bg-neutral-200 text-neutral-700 text-sm font-medium rounded-md hover:bg-neutral-300 transition-colors"
          >
            Test Too Long (INTERNAL_ERROR)
          </button>
        </div>
      </div>

      {mutation.error && (
        <div className="p-4 bg-red-50 border border-red-200 rounded-lg">
          {mutation.error instanceof ORPCError ? (
            <div className="space-y-2">
              <p className="font-semibold text-red-900">{mutation.error.code}</p>
              <p className="text-red-700 text-sm">{mutation.error.message || String(mutation.error.data)}</p>
              
              <div className="bg-neutral-900 rounded p-3 font-mono text-xs text-red-400 overflow-x-auto">
                <pre className="whitespace-pre-wrap break-words">{JSON.stringify(mutation.error, null, 2)}</pre>
              </div>
            </div>
          ) : (
            <div className="space-y-2">
              <p className="text-red-800">Unexpected error type</p>
              <div className="bg-neutral-900 rounded p-3 font-mono text-xs text-red-400 overflow-x-auto">
                <pre className="whitespace-pre-wrap break-words">{JSON.stringify(mutation.error, null, 2)}</pre>
              </div>
            </div>
          )}
        </div>
      )}

      {mutation.data && (
        <div className="p-4 bg-emerald-50 border border-emerald-200 rounded-lg">
          <p className="text-emerald-800 font-medium">
            ✓ Created: {mutation.data.name} (#{mutation.data.id})
          </p>
        </div>
      )}
    </div>
  );
}
