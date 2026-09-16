import { ArrowDownUp, Check } from "lucide-react";

import { Button } from "@/components/ui/Button";
import { Menu } from "@/components/ui/Menu";
import { SORT_LABEL, type SortKey } from "@/features/projects/sort";
import { useUi } from "@/state/ui";

/**
 * Sorting, where Apple puts it: a toolbar menu, not a clickable column header.
 *
 * A grouped list has no header row, so the control has to live somewhere else
 * — and Files, Photos and Mail all answer that the same way, with a sort menu
 * in the toolbar whose current choice carries a checkmark. Picking the
 * selected key again flips the direction, which is the same three-state
 * behaviour the old headers had (sort, reverse, clear) minus the clear: the
 * "Default" item does that explicitly instead of being an invisible third
 * click.
 */
const KEYS: SortKey[] = ["name", "status", "kind", "launched", "used", "added"];

export function SortMenu() {
  const sort = useUi((s) => s.sort);
  const toggleSort = useUi((s) => s.toggleSort);

  const label = sort
    ? `Sorted by ${SORT_LABEL[sort.key]}, ${sort.dir === "asc" ? "ascending" : "descending"}`
    : "Sort: default order";

  return (
    <Menu
      align="end"
      // NO Tooltip around this trigger. Radix's menu trigger clones its props
      // onto its single child; wrapping the button in a Tooltip means the
      // Tooltip receives them instead, and the button never gets
      // aria-haspopup or the pointerdown handler -- measured: the menu simply
      // did not open. The aria-label carries the same information a tooltip
      // would have, and RowActions' working menu has no tooltip either.
      trigger={
        <Button size="icon" aria-label={label} data-sort-menu>
          <ArrowDownUp size={15} />
        </Button>
      }
      items={[
        ...KEYS.map((key) => ({
          label: SORT_LABEL[key],
          // The checkmark is the selection, the way an iOS menu shows it.
          // An arrow beside it would say the same thing twice.
          icon:
            sort?.key === key ? (
              <Check size={13} className="text-accent" />
            ) : (
              <span className="inline-block w-[13px]" aria-hidden />
            ),
          onSelect: () => toggleSort(key),
        })),
        {
          label: "Default order",
          section: true,
          icon:
            sort === null ? (
              <Check size={13} className="text-accent" />
            ) : (
              <span className="inline-block w-[13px]" aria-hidden />
            ),
          // `toggleSort` clears on the third press of the same key; calling it
          // twice reaches the cleared state from anywhere.
          onSelect: () => {
            const current = useUi.getState().sort;
            if (!current) return;
            toggleSort(current.key);
            if (useUi.getState().sort) toggleSort(current.key);
          },
        },
      ]}
    />
  );
}
