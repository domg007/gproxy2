import type { ReactNode } from "react";
import { Card, CardContent } from "@/components/ui/card";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import { cn } from "@/lib/utils";

export interface DataColumn<T> {
  key: string;
  header: ReactNode;
  cell: (row: T) => ReactNode;
  className?: string;
}

interface DataTableProps<T> {
  columns: DataColumn<T>[];
  rows: T[];
  rowKey: (row: T) => string | number;
  /** Mobile (<md) rendering — required so every list ships its phone form (spec §4). */
  renderCard: (row: T) => ReactNode;
  empty: ReactNode;
  onRowClick?: (row: T) => void;
}

export function DataTable<T>({ columns, rows, rowKey, renderCard, empty, onRowClick }: DataTableProps<T>) {
  if (rows.length === 0) {
    return <div className="rounded-md border p-8 text-center text-sm text-muted-foreground">{empty}</div>;
  }
  const clickable = onRowClick !== undefined;
  return (
    <>
      <div className="hidden rounded-md border md:block">
        <Table>
          <TableHeader>
            <TableRow>
              {columns.map((col) => (
                <TableHead key={col.key} className={col.className}>{col.header}</TableHead>
              ))}
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <TableRow
                key={rowKey(row)}
                className={cn(clickable && "cursor-pointer")}
                onClick={() => onRowClick?.(row)}
              >
                {columns.map((col) => (
                  <TableCell key={col.key} className={col.className}>{col.cell(row)}</TableCell>
                ))}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
      <div className="grid gap-2 md:hidden">
        {rows.map((row) => (
          <Card
            key={rowKey(row)}
            className={cn(clickable && "cursor-pointer")}
            onClick={() => onRowClick?.(row)}
          >
            <CardContent className="p-4">{renderCard(row)}</CardContent>
          </Card>
        ))}
      </div>
    </>
  );
}
