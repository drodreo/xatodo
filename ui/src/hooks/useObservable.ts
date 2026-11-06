/**
 * useObservable: the only bridge for components to subscribe to service-layer
 * streams.
 * Subscribing to a BehaviorSubject replays the current value synchronously,
 * so real state is available right after the first frame.
 * Components must never assemble data with their own fetch + useState; all
 * remote state is subscribed from here.
 */
import { useEffect, useState } from 'react';
import type { Observable } from 'rxjs';

export function useObservable<T>(source$: Observable<T>, initialValue: T): T {
  const [value, setValue] = useState<T>(initialValue);

  useEffect(() => {
    const subscription = source$.subscribe(setValue);
    return () => subscription.unsubscribe();
    // Service-layer streams are module-level singletons with stable
    // references; depending on source$ alone is sufficient here
  }, [source$]);

  return value;
}
