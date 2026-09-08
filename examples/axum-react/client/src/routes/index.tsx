import {
  client,
  consumeAsyncIterator,
  getEventMeta,
  isInferableError,
  orpc,
} from "@/rpc/better-auth-contract";
import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { useEffect, useRef, useState } from "react";

export const Route = createFileRoute("/")({ component: Home });

function Home() {
  return (
    <div className="min-h-screen bg-neutral-50">
      {/* Header */}
      <header className="border-b border-neutral-200 bg-white">
        <div className="max-w-7xl mx-auto px-6 py-6">
          <div className="flex items-baseline gap-3">
            <h1 className="text-2xl font-semibold tracking-tight text-neutral-900">
              Planet Explorer
            </h1>
            <span className="text-sm text-neutral-400 font-mono">
              oRPC Demo
            </span>
          </div>
        </div>
      </header>

      <div className="max-w-7xl mx-auto px-6 py-12">
        {/* Connection Section */}
        <section className="mb-16">
          <PingTest />
        </section>

        {/* CRUD Operations */}
        <section className="mb-16">
          <h2 className="text-xs font-medium uppercase tracking-wider text-neutral-500 mb-6">
            CRUD Operations
          </h2>
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
            <PlanetList />
            <div className="space-y-6">
              <PlanetFind />
              <CreatePlanet />
              <DeletePlanet />
            </div>
          </div>
        </section>

        {/* Pagination Demo */}
        <section className="mb-16">
          <h2 className="text-xs font-medium uppercase tracking-wider text-neutral-500 mb-6">
            Pagination Demo
          </h2>
          <PlanetListInfinite />
        </section>

        {/* Streaming Section */}
        <section>
          <h2 className="text-xs font-medium uppercase tracking-wider text-neutral-500 mb-6">
            Server-Sent Events Streaming
          </h2>
          <div className="grid grid-cols-1 gap-6 mb-6">
            <StreamEvents />
          </div>
          <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
            <StreamAsyncConsumeIterator />
            <StreamAsyncStreamed />
            <StreamAsyncLive />
          </div>
        </section>
      </div>
    </div>
  );
}

function PingTest() {
  const mutation = useMutation(orpc.ping.mutationOptions());

  return (
    <div className="bg-white border border-neutral-200 rounded-lg p-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-4">
          <button
            onClick={() => mutation.mutate(undefined)}
            disabled={mutation.isPending}
            className="px-5 py-2.5 bg-neutral-900 text-white text-sm font-medium rounded-md hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
          >
            {mutation.isPending ? "Testing…" : "Test Connection"}
          </button>
          {mutation.data && (
            <div className="flex items-center gap-2">
              <div className="w-2 h-2 bg-emerald-500 rounded-full"></div>
              <span className="text-sm font-mono text-neutral-600">
                {mutation.data.id} {" ... "} {mutation.data.message}
              </span>
            </div>
          )}
          {mutation.error && (
            <span className="text-sm text-red-600">Connection failed</span>
          )}
        </div>
      </div>
    </div>
  );
}

function PlanetList() {
  const {
    data: planets,
    isLoading,
    error,
    refetch,
  } = useQuery(orpc.planet.listPlanets.queryOptions());

  if (isLoading) {
    return (
      <div className="bg-white border border-neutral-200 rounded-lg p-8">
        <div className="animate-pulse space-y-3">
          <div className="h-4 bg-neutral-100 rounded w-32"></div>
          <div className="h-16 bg-neutral-100 rounded"></div>
          <div className="h-16 bg-neutral-100 rounded"></div>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="bg-white border border-red-200 rounded-lg p-8">
        <p className="text-sm text-red-600">{String(error)}</p>
      </div>
    );
  }

  return (
    <div className="bg-white border border-neutral-200 rounded-lg overflow-hidden">
      <div className="px-6 py-4 border-b border-neutral-200 flex items-center justify-between">
        <h3 className="text-sm font-semibold text-neutral-900">All Planets</h3>
        <button
          onClick={() => refetch()}
          className="text-xs text-neutral-500 hover:text-neutral-900 font-medium transition-colors"
        >
          Refresh
        </button>
      </div>

      <div className="divide-y divide-neutral-100">
        {planets?.map((planet) => (
          <div
            key={planet.id}
            className="px-6 py-4 hover:bg-neutral-50 transition-colors"
          >
            <div className="flex items-start justify-between">
              <div>
                <h4 className="text-sm font-medium text-neutral-900">
                  {planet.name}
                </h4>
                {planet.description && (
                  <p className="text-sm text-neutral-500 mt-1">
                    {planet.description}
                  </p>
                )}
              </div>
              <span className="text-xs font-mono text-neutral-400">
                #{planet.id}
              </span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function PlanetFind() {
  const [id, setId] = useState<number>(1);

  const {
    data: planet,
    isLoading,
    error,
  } = useQuery(orpc.planet.findPlanet.queryOptions({ input: { id } }));

  return (
    <div className="bg-white border border-neutral-200 rounded-lg p-6">
      <h3 className="text-sm font-semibold text-neutral-900 mb-4">
        Find by ID
      </h3>

      <div className="flex items-center gap-3 mb-4">
        <input
          type="number"
          value={id}
          min={1}
          onChange={(e) => setId(Number(e.target.value))}
          className="w-20 px-3 py-2 text-sm border border-neutral-300 rounded-md focus:outline-none focus:ring-2 focus:ring-neutral-900 focus:border-transparent"
        />
        <button
          onClick={() => setId(999)}
          className="px-3 py-2 text-xs bg-red-100 text-red-700 rounded-md hover:bg-red-200 transition-colors"
        >
          Test Store Error (999)
        </button>
      </div>

      {isLoading && (
        <div className="animate-pulse">
          <div className="h-16 bg-neutral-100 rounded"></div>
        </div>
      )}

      {error && (
        <div className="p-3 bg-red-50 border border-red-200 rounded text-sm text-red-800">
          {isInferableError(error) ? (
            <>
              {error.code === "NOT_FOUND" && (
                <div className="flex items-start gap-2">
                  <span className="text-red-600">⚠️</span>
                  <div>
                    <p className="font-medium">Planet Not Found</p>
                    <p className="text-red-700 mt-1">{error.message}</p>
                  </div>
                </div>
              )}
              {error.code !== "NOT_FOUND" && <p>{error.message}</p>}
            </>
          ) : (
            <p>An unexpected error occurred: {String(error)}</p>
          )}
        </div>
      )}

      {planet && !isLoading && (
        <div className="p-4 bg-neutral-50 border border-neutral-200 rounded-lg">
          <div className="flex items-start justify-between mb-1">
            <h4 className="text-sm font-medium text-neutral-900">
              {planet.name}
            </h4>
            <span className="text-xs font-mono text-neutral-400">
              #{planet.id}
            </span>
          </div>
          {planet.description && (
            <p className="text-sm text-neutral-600 mt-2">
              {planet.description}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

function CreatePlanet() {
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");

  const mutation = useMutation(
    orpc.planet.createPlanet.mutationOptions({
      onSuccess: () => {
        qc.invalidateQueries({ queryKey: orpc.planet.key() });
        setName("");
        setDescription("");
      },
    }),
  );

  const handleCreate = (e: React.FormEvent) => {
    e.preventDefault();
    mutation.mutate({ name, description: description || undefined });
  };

  return (
    <div className="bg-white border border-neutral-200 rounded-lg p-6">
      <h3 className="text-sm font-semibold text-neutral-900 mb-4">
        Create Planet
      </h3>

      <form onSubmit={handleCreate} className="space-y-4">
        <div>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="w-full px-3 py-2 text-sm border border-neutral-300 rounded-md placeholder:text-neutral-400 focus:outline-none focus:ring-2 focus:ring-neutral-900 focus:border-transparent"
            placeholder="Planet name"
          />
        </div>

        <div>
          <textarea
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            rows={2}
            className="w-full px-3 py-2 text-sm border border-neutral-300 rounded-md placeholder:text-neutral-400 focus:outline-none focus:ring-2 focus:ring-neutral-900 focus:border-transparent resize-none"
            placeholder="Description (optional)"
          />
        </div>

        <button
          type="submit"
          disabled={mutation.isPending}
          className="w-full px-4 py-2.5 bg-neutral-900 text-white text-sm font-medium rounded-md hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          {mutation.isPending ? "Creating…" : "Create"}
        </button>
      </form>

      {mutation.data && (
        <div className="mt-4 p-3 bg-emerald-50 border border-emerald-200 rounded-md">
          <p className="text-sm text-emerald-800 font-medium">
            Created {mutation.data.name}
          </p>
        </div>
      )}

      {mutation.error && (
        <div className="mt-4 p-3 bg-red-50 border border-red-200 rounded-md">
          {isInferableError(mutation.error) ? (
            <>
              {mutation.error.code === "INTERNAL" && (
                <div className="flex items-start gap-2 text-red-800">
                  <span className="text-red-600">❌</span>
                  <div>
                    <p className="font-medium">Validation Error</p>
                    <p className="text-red-700 mt-1">
                      {mutation.error.message}
                    </p>
                  </div>
                </div>
              )}
              {mutation.error.code !== "INTERNAL" && (
                <p className="text-red-800">{mutation.error.message}</p>
              )}
            </>
          ) : (
            <p className="text-red-800">
              An unexpected error occurred: {String(mutation.error)}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

function DeletePlanet() {
  const qc = useQueryClient();
  const [id, setId] = useState<number>(1);

  const mutation = useMutation(
    orpc.planet.deletePlanet.mutationOptions({
      onSuccess: () => {
        qc.invalidateQueries({ queryKey: orpc.planet.key() });
      },
    }),
  );

  const handleDelete = (e: React.FormEvent) => {
    e.preventDefault();
    mutation.mutate({ id });
  };

  return (
    <div className="bg-white border border-neutral-200 rounded-lg p-6">
      <h3 className="text-sm font-semibold text-neutral-900 mb-4">
        Delete Planet
      </h3>

      <form onSubmit={handleDelete} className="space-y-4">
        <div>
          <input
            type="number"
            value={id}
            min={1}
            onChange={(e) => setId(Number(e.target.value))}
            className="w-full px-3 py-2 text-sm border border-neutral-300 rounded-md placeholder:text-neutral-400 focus:outline-none focus:ring-2 focus:ring-red-600 focus:border-transparent"
            placeholder="Planet ID"
          />
        </div>

        <button
          type="submit"
          disabled={mutation.isPending}
          className="w-full px-4 py-2.5 bg-red-600 text-white text-sm font-medium rounded-md hover:bg-red-700 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          {mutation.isPending ? "Deleting…" : "Delete"}
        </button>
      </form>

      {mutation.isSuccess && (
        <div className="mt-4 p-3 bg-emerald-50 border border-emerald-200 rounded-md">
          <p className="text-sm text-emerald-800 font-medium">
            Planet #{id} deleted successfully
          </p>
        </div>
      )}

      {mutation.error && (
        <div className="mt-4 p-3 bg-red-50 border border-red-200 rounded-md">
          {isInferableError(mutation.error) ? (
            <>
              {mutation.error.code === "NOT_FOUND" && (
                <div className="flex items-start gap-2 text-red-800">
                  <span className="text-red-600">⚠️</span>
                  <div>
                    <p className="font-medium">Planet Not Found</p>
                    <p className="text-red-700 mt-1">
                      {mutation.error.message}
                    </p>
                  </div>
                </div>
              )}
            </>
          ) : (
            <p className="text-red-800">
              An unexpected error occurred: {String(mutation.error)}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

function PlanetListInfinite() {
  const {
    data,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isLoading,
    error,
  } = useInfiniteQuery(
    orpc.planet.listPlanetsPaginated.infiniteOptions({
      input: (pageParam: number | undefined) => ({
        limit: 5,
        offset: pageParam ?? 0,
      }),
      initialPageParam: undefined,
      getNextPageParam: (lastPage) => lastPage.next_page_param,
    }),
  );

  if (isLoading) {
    return (
      <div className="bg-white border border-neutral-200 rounded-lg p-8">
        <div className="animate-pulse space-y-3">
          <div className="h-4 bg-neutral-100 rounded w-32"></div>
          <div className="h-16 bg-neutral-100 rounded"></div>
          <div className="h-16 bg-neutral-100 rounded"></div>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="bg-white border border-red-200 rounded-lg p-8">
        <p className="text-sm text-red-600">{String(error)}</p>
      </div>
    );
  }

  const allPlanets = data?.pages.flatMap((page) => page.items) ?? [];

  return (
    <div className="bg-white border border-neutral-200 rounded-lg overflow-hidden">
      <div className="px-6 py-4 border-b border-neutral-200 flex items-center justify-between">
        <div>
          <h3 className="text-sm font-semibold text-neutral-900">
            Infinite Query (Pagination)
          </h3>
          <p className="text-xs text-neutral-500 mt-0.5">
            {allPlanets.length} planets loaded • {data?.pages.length ?? 0} pages
          </p>
        </div>
        <code className="text-xs font-mono text-neutral-400">
          infiniteOptions
        </code>
      </div>

      <div className="divide-y divide-neutral-100">
        {allPlanets.map((planet) => (
          <div
            key={planet.id}
            className="px-6 py-4 hover:bg-neutral-50 transition-colors"
          >
            <div className="flex items-start justify-between">
              <div>
                <h4 className="text-sm font-medium text-neutral-900">
                  {planet.name}
                </h4>
                {planet.description && (
                  <p className="text-sm text-neutral-500 mt-1">
                    {planet.description}
                  </p>
                )}
              </div>
              <span className="text-xs font-mono text-neutral-400">
                #{planet.id}
              </span>
            </div>
          </div>
        ))}
      </div>

      {hasNextPage && (
        <div className="px-6 py-4 border-t border-neutral-200 bg-neutral-50">
          <button
            onClick={() => fetchNextPage()}
            disabled={isFetchingNextPage}
            className="w-full px-4 py-2.5 bg-neutral-900 text-white text-sm font-medium rounded-md hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
          >
            {isFetchingNextPage ? "Loading..." : "Load More"}
          </button>
        </div>
      )}

      {!hasNextPage && allPlanets.length > 0 && (
        <div className="px-6 py-4 border-t border-neutral-200 bg-neutral-50">
          <p className="text-sm text-neutral-500 text-center">
            All planets loaded
          </p>
        </div>
      )}
    </div>
  );
}

function StreamEvents() {
  const [events, setEvents] = useState<
    { message: string; count: number; id?: string; retry?: number }[]
  >([]);
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string>("");
  const abortRef = useRef<AbortController | null>(null);

  const handleStart = async () => {
    setEvents([]);
    setError("");
    setStreaming(true);

    const controller = new AbortController();
    abortRef.current = controller;

    try {
      const iterator = await client.streamEvents(undefined, {
        signal: controller.signal,
      });
      for await (const event of iterator) {
        const meta = getEventMeta(event);
        setEvents((prev) => [
          ...prev,
          { ...event, id: meta?.id, retry: meta?.retry },
        ]);
      }
    } catch (err: unknown) {
      if (err instanceof Error && err.name !== "AbortError") {
        setError(String(err));
      }
    } finally {
      setStreaming(false);
      abortRef.current = null;
    }
  };

  const handleStop = () => {
    abortRef.current?.abort();
  };

  // cleanup on unmount
  useEffect(() => () => abortRef.current?.abort(), []);

  return (
    <div className="bg-white border border-neutral-200 rounded-lg p-6">
      <div className="flex items-baseline justify-between mb-6">
        <div>
          <h3 className="text-lg font-semibold text-neutral-900">
            Basic SSE Stream
          </h3>
          <p className="text-sm text-neutral-500 mt-1">
            for await...of pattern • manual AbortController
          </p>
        </div>
        <code className="text-xs font-mono text-neutral-400">tokio_stream</code>
      </div>

      <div className="flex gap-2 mb-6">
        <button
          onClick={handleStart}
          disabled={streaming}
          className="px-4 py-2 bg-neutral-900 text-white text-sm font-medium rounded hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          {streaming ? "Streaming" : "Start"}
        </button>
        <button
          onClick={handleStop}
          disabled={!streaming}
          className="px-4 py-2 bg-white border border-neutral-300 text-neutral-700 text-sm font-medium rounded hover:border-neutral-400 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          Cancel
        </button>
      </div>

      {error && (
        <div className="mb-4 p-3 bg-red-50 border border-red-200 rounded text-sm text-red-800">
          {error}
        </div>
      )}

      <div className="space-y-0.5 max-h-64 overflow-y-auto">
        {events.map((e, idx) => (
          <div
            key={idx}
            className="flex items-center gap-3 p-2 font-mono text-xs bg-neutral-50 text-neutral-700 rounded border border-transparent hover:border-neutral-200 transition-colors"
          >
            <span className="text-neutral-400 tabular-nums w-6 text-right">
              {e.count}
            </span>
            <span className="flex-1">{e.message}</span>
            {e.id && (
              <span className="text-neutral-400 text-[10px]">
                id: {e.id} • retry: {e.retry}ms
              </span>
            )}
          </div>
        ))}
        {streaming && events.length > 0 && (
          <div className="p-2 text-xs text-neutral-400 font-mono animate-pulse">
            ▸ waiting
          </div>
        )}
        {!streaming && events.length === 0 && (
          <p className="text-sm text-neutral-400 py-8 text-center">
            No events. Press Start.
          </p>
        )}
      </div>
    </div>
  );
}

// consumeAsyncIterator pattern
function StreamAsyncConsumeIterator() {
  const [events, setEvents] = useState<
    { message: string; count: number; id?: string; retry?: number }[]
  >([]);
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string>("");
  const [finished, setFinished] = useState(false);
  const cancelRef = useRef<(() => Promise<void>) | null>(null);

  const handleStart = () => {
    setEvents([]);
    setError("");
    setFinished(false);
    setStreaming(true);

    const cancel = consumeAsyncIterator(client.streamEventsAsync(), {
      onEvent: (event) => {
        const meta = getEventMeta(event);
        setEvents((prev) => [
          ...prev,
          { ...event, id: meta?.id, retry: meta?.retry },
        ]);
      },
      onError: (err) => {
        setError(String(err));
        setStreaming(false);
      },
      onSuccess: (value) => {
        console.log("Stream completed successfully:", value);
        setStreaming(false);
      },
      onFinish: (state) => {
        console.log("Stream finished with state:", state);
        setFinished(true);
        setStreaming(false);
        cancelRef.current = null;
      },
    });

    cancelRef.current = cancel;
  };

  const handleStop = async () => {
    if (cancelRef.current) {
      await cancelRef.current();
    }
  };

  // cleanup on unmount
  useEffect(() => {
    return () => {
      if (cancelRef.current) {
        void cancelRef.current();
      }
    };
  }, []);

  return (
    <div className="bg-white border border-neutral-200 rounded-lg p-6">
      <div className="flex items-baseline justify-between mb-6">
        <div>
          <h2 className="text-xl font-semibold text-neutral-900">
            consumeAsyncIterator
          </h2>
          <p className="text-sm text-neutral-500 mt-1">
            Lifecycle callbacks • manual cancellation
          </p>
        </div>
        <code className="text-xs font-mono text-neutral-400">
          async_stream::stream!
        </code>
      </div>

      <div className="flex gap-2 mb-6">
        <button
          onClick={handleStart}
          disabled={streaming}
          className="px-4 py-2 bg-neutral-900 text-white text-sm font-medium rounded hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          {streaming ? "Streaming" : "Start"}
        </button>
        <button
          onClick={handleStop}
          disabled={!streaming}
          className="px-4 py-2 bg-white border border-neutral-300 text-neutral-700 text-sm font-medium rounded hover:border-neutral-400 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          Cancel
        </button>
      </div>

      {error && (
        <div className="mb-4 p-3 bg-red-50 border border-red-200 rounded text-sm text-red-800">
          {error}
        </div>
      )}

      {finished && !streaming && (
        <div className="mb-4 p-3 bg-green-50 border border-green-200 rounded text-sm text-green-800">
          Stream completed
        </div>
      )}

      <div className="space-y-0.5 max-h-64 overflow-y-auto">
        {events.map((e, idx) => (
          <div
            key={idx}
            className="flex items-center gap-3 p-2 font-mono text-xs bg-neutral-50 text-neutral-700 rounded border border-transparent hover:border-neutral-200 transition-colors"
          >
            <span className="text-neutral-400 tabular-nums w-6 text-right">
              {e.count}
            </span>
            <span className="flex-1">{e.message}</span>
            {e.id && (
              <span className="text-neutral-400 text-[10px]">
                id: {e.id} • retry: {e.retry}ms
              </span>
            )}
          </div>
        ))}
        {streaming && events.length > 0 && (
          <div className="p-2 text-xs text-neutral-400 font-mono animate-pulse">
            ▸ waiting
          </div>
        )}
        {!streaming && events.length === 0 && (
          <p className="text-sm text-neutral-400 py-8 text-center">
            No events. Press Start.
          </p>
        )}
      </div>
    </div>
  );
}

// streamedOptions pattern — array, each event appended
function StreamAsyncStreamed() {
  const {
    data: events,
    isLoading,
    error,
    refetch,
  } = useQuery(orpc.streamEventsAsync.streamedOptions({ retry: false }));

  return (
    <div className="bg-white border border-neutral-200 rounded-lg p-6">
      <div className="flex items-baseline justify-between mb-6">
        <div>
          <h2 className="text-xl font-semibold text-neutral-900">
            streamedOptions
          </h2>
          <p className="text-sm text-neutral-500 mt-1">
            TanStack Query • accumulated array
          </p>
        </div>
        <code className="text-xs font-mono text-neutral-400">Event[]</code>
      </div>

      <div className="mb-6">
        <button
          onClick={() => refetch()}
          disabled={isLoading}
          className="px-4 py-2 bg-neutral-900 text-white text-sm font-medium rounded hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          {isLoading ? "Streaming" : "Start"}
        </button>
      </div>

      {error && (
        <div className="mb-4 p-3 bg-red-50 border border-red-200 rounded text-sm text-red-800">
          {String(error)}
        </div>
      )}

      <div className="space-y-0.5 max-h-64 overflow-y-auto">
        {events?.map((e, idx) => {
          const meta = getEventMeta(e);
          return (
            <div
              key={idx}
              className="flex items-center gap-3 p-2 font-mono text-xs bg-neutral-50 text-neutral-700 rounded border border-transparent hover:border-neutral-200 transition-colors"
            >
              <span className="text-neutral-400 tabular-nums w-6 text-right">
                {e.count}
              </span>
              <span className="flex-1">{e.message}</span>
              {meta?.id && (
                <span className="text-neutral-400 text-[10px]">
                  id: {meta.id} • retry: {meta.retry}ms
                </span>
              )}
            </div>
          );
        })}
        {!isLoading && (!events || events.length === 0) && (
          <p className="text-sm text-neutral-400 py-8 text-center">
            No events. Press Start.
          </p>
        )}
      </div>
    </div>
  );
}

// liveOptions pattern — always the latest event only
function StreamAsyncLive() {
  const {
    data: latest,
    isLoading,
    error,
    refetch,
  } = useQuery(orpc.streamEventsAsync.liveOptions({ retry: false }));

  return (
    <div className="bg-white border border-neutral-200 rounded-lg p-6">
      <div className="flex items-baseline justify-between mb-6">
        <div>
          <h2 className="text-xl font-semibold text-neutral-900">
            liveOptions
          </h2>
          <p className="text-sm text-neutral-500 mt-1">
            TanStack Query • latest value only
          </p>
        </div>
        <code className="text-xs font-mono text-neutral-400">
          Event | undefined
        </code>
      </div>

      <div className="mb-6">
        <button
          onClick={() => refetch()}
          disabled={isLoading}
          className="px-4 py-2 bg-neutral-900 text-white text-sm font-medium rounded hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          {isLoading ? "Streaming" : "Start"}
        </button>
      </div>

      {error && (
        <div className="mb-4 p-3 bg-red-50 border border-red-200 rounded text-sm text-red-800">
          {String(error)}
        </div>
      )}

      <div>
        {latest ? (
          <>
            <div className="p-4 bg-neutral-900 text-white rounded-lg border-2 border-neutral-300">
              <div className="flex items-center gap-2 mb-3 text-xs font-mono text-neutral-400">
                <span className="inline-block w-2 h-2 bg-green-400 rounded-full animate-pulse"></span>
                <span>LIVE</span>
              </div>
              <div className="flex items-center gap-3 font-mono text-sm">
                <span className="text-neutral-400 tabular-nums">
                  {latest.count}
                </span>
                <span className="flex-1">{latest.message}</span>
              </div>
            </div>
            {(() => {
              const meta = getEventMeta(latest);
              return meta?.id ? (
                <div className="mt-2 text-xs text-neutral-400 font-mono">
                  Event metadata: id: {meta.id} • retry: {meta.retry}ms
                </div>
              ) : null;
            })()}
          </>
        ) : (
          <p className="text-sm text-neutral-400 py-8 text-center">
            {isLoading ? "Waiting for first event…" : "No event. Press Start."}
          </p>
        )}
        {isLoading && latest && (
          <div className="mt-2 text-xs text-neutral-400 font-mono animate-pulse">
            ▸ waiting for update
          </div>
        )}
      </div>
    </div>
  );
}
