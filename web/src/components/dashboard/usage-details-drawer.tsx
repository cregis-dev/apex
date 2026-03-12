"use client";

import { X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { UsageRecord } from "@/components/dashboard/types";

type UsageDetailsDrawerProps = {
  record: UsageRecord | null;
  onClose: () => void;
};

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid gap-1 rounded-2xl bg-[#f8f0e1] px-4 py-3">
      <div className="text-xs font-medium uppercase tracking-[0.18em] text-[#7a6957]">{label}</div>
      <div className="break-all text-sm text-[#2f1e16]">{value || "—"}</div>
    </div>
  );
}

export function UsageDetailsDrawer({ record, onClose }: UsageDetailsDrawerProps) {
  if (!record) {
    return null;
  }

  return (
    <div className="fixed inset-0 z-50 flex justify-end bg-[#2b1c14]/20 backdrop-blur-sm">
      <button
        type="button"
        aria-label="Close details"
        className="flex-1"
        onClick={onClose}
      />
      <div className="relative h-full w-full max-w-xl overflow-y-auto border-l border-[#d9ccb7] bg-[#fff9f0] p-4 shadow-[0_20px_50px_-30px_rgba(66,31,15,0.35)]">
        <div className="mb-4 flex items-start justify-between gap-4">
          <div>
            <div className="text-xs font-medium uppercase tracking-[0.22em] text-[#7a6957]">
              Request Details
            </div>
            <h2 className="mt-1 text-2xl font-semibold text-[#24140f]">
              {record.request_id ?? `record-${record.id}`}
            </h2>
          </div>
          <Button variant="outline" size="icon-sm" onClick={onClose} aria-label="Close details drawer">
            <X className="size-4" />
          </Button>
        </div>

        <Card className="border-[#d9ccb7] bg-[#fff9f0]">
          <CardHeader>
            <CardTitle>Routing & Status</CardTitle>
          </CardHeader>
          <CardContent className="grid gap-3">
            <DetailRow label="Time" value={record.timestamp} />
            <DetailRow label="Team" value={record.team_id} />
            <DetailRow label="Router" value={record.router} />
            <DetailRow label="Matched Rule" value={record.matched_rule ?? "fallback"} />
            <DetailRow label="Final Channel" value={record.final_channel} />
            <DetailRow label="Model" value={record.model} />
            <DetailRow label="Status" value={`${record.status}${record.status_code ? ` (${record.status_code})` : ""}`} />
            <DetailRow label="Latency" value={record.latency_ms ? `${record.latency_ms.toFixed(1)} ms` : "n/a"} />
            <DetailRow
              label="Tokens"
              value={`in ${record.input_tokens.toLocaleString()} / out ${record.output_tokens.toLocaleString()}`}
            />
            <DetailRow label="Provider Trace" value={record.provider_trace_id ?? ""} />
            <DetailRow label="Error Message" value={record.error_message ?? ""} />
            <DetailRow label="Provider Error Body" value={record.provider_error_body ?? ""} />
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
