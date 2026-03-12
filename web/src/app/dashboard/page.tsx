import { Suspense } from "react";
import DashboardClient from "@/components/dashboard/dashboard-client";

export default function DashboardPage() {
  return (
    <Suspense fallback={<div className="min-h-screen bg-gray-50 p-8" />}>
      <DashboardClient />
    </Suspense>
  );
}
