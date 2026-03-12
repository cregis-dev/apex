import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Apex Gateway Dashboard",
  description: "AI Gateway Usage Dashboard",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className="antialiased">
        {children}
      </body>
    </html>
  );
}
