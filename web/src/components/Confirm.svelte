<script lang="ts">
  let {
    title,
    body,
    confirmLabel = 'Confirm',
    danger = false,
    onConfirm,
    onCancel
  } = $props<{
    title: string;
    body: string;
    confirmLabel?: string;
    danger?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  }>();
</script>

<div class="confirm-bg" role="presentation" onclick={(e) => { if (e.currentTarget === e.target) onCancel(); }}>
  <div class="confirm-card" role="dialog" aria-label={title}>
    <h3>{title}</h3>
    <p>{body}</p>
    <div class="row">
      <button class="btn secondary" onclick={onCancel}>Cancel</button>
      <button class="btn" class:danger onclick={onConfirm}>{confirmLabel}</button>
    </div>
  </div>
</div>
