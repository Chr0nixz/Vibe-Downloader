import { motion, AnimatePresence } from "framer-motion";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { Task } from "@/types/task";
import { formatEta, formatPercent, formatSpeed } from "@/lib/utils";
import { cn } from "@/lib/utils";

export function TaskDetails({ task, open }: { task: Task | null; open: boolean }) {
  return (
    <AnimatePresence>
      {open && task ? (
        <motion.aside
          initial={{ opacity: 0, x: 16 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: 16 }}
          transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
          className="flex w-80 shrink-0 flex-col border-l border-border-subtle bg-surface-base xl:w-96"
        >
          <header className="border-b border-border-subtle px-4 py-3">
            <h2 className="truncate text-sm font-medium">{task.fileName}</h2>
            <p className="truncate text-xs text-text-muted">{task.saveDir}</p>
          </header>
          <Tabs defaultValue="overview" className="flex min-h-0 flex-1 flex-col px-4 py-3">
            <TabsList>
              <TabsTrigger value="overview">Overview</TabsTrigger>
              <TabsTrigger value="chunks">Chunks</TabsTrigger>
              <TabsTrigger value="connections">Connections</TabsTrigger>
            </TabsList>
            <ScrollArea className="min-h-0 flex-1">
              <TabsContent value="overview" className="space-y-2 text-sm">
                <Row label="Progress" value={formatPercent(task.downloadedBytes, task.totalSize)} />
                <Row label="Speed" value={formatSpeed(task.speedBps)} />
                <Row label="ETA" value={formatEta(task.downloadedBytes, task.totalSize, task.speedBps)} />
              </TabsContent>
              <TabsContent value="chunks">
                <p className="text-xs text-text-secondary">Chunk heatmap placeholder for HTTP MVP.</p>
              </TabsContent>
              <TabsContent value="connections">
                <p className="text-xs text-text-muted">Connections tab placeholder.</p>
              </TabsContent>
            </ScrollArea>
          </Tabs>
        </motion.aside>
      ) : null}
    </AnimatePresence>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-xs text-text-muted">{label}</div>
      <div className={cn("text-text-primary")}>{value}</div>
    </div>
  );
}
