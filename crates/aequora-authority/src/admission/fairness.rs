use std::collections::{BTreeMap, BTreeSet, VecDeque};

use aequora_types::TenantId;
use aequora_types::scheduler::WorkClass;
use serde::{Deserialize, Serialize};

use crate::admission::{PolicyError, QueueRejection, WorkDescriptor};

const CLASSES: [WorkClass; 6] = [
    WorkClass::Critical,
    WorkClass::Interactive,
    WorkClass::Normal,
    WorkClass::Bulk,
    WorkClass::Background,
    WorkClass::Maintenance,
];

/// Explicit queue bounds and weighted class schedule.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FairQueueConfig {
    pub max_queued: usize,
    pub max_queued_per_tenant: usize,
    pub max_queued_bytes: u64,
    pub critical_capacity: usize,
    pub interactive_capacity: usize,
    pub normal_capacity: usize,
    pub bulk_capacity: usize,
    pub background_capacity: usize,
    pub maintenance_capacity: usize,
    pub critical_weight: u8,
    pub interactive_weight: u8,
    pub normal_weight: u8,
    pub bulk_weight: u8,
    pub background_weight: u8,
    pub maintenance_weight: u8,
    pub aging_quantum_ms: u64,
}

impl Default for FairQueueConfig {
    fn default() -> Self {
        Self {
            max_queued: 512,
            max_queued_per_tenant: 64,
            max_queued_bytes: 256 * 1_024 * 1_024,
            critical_capacity: 64,
            interactive_capacity: 192,
            normal_capacity: 192,
            bulk_capacity: 32,
            background_capacity: 24,
            maintenance_capacity: 8,
            critical_weight: 8,
            interactive_weight: 6,
            normal_weight: 4,
            bulk_weight: 2,
            background_weight: 1,
            maintenance_weight: 1,
            aging_quantum_ms: 1_000,
        }
    }
}

impl FairQueueConfig {
    /// Validates every global, tenant, class, byte, weight, and aging bound.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::InvalidQueuePolicy`] for zero or inconsistent bounds.
    pub const fn validate(self) -> Result<(), PolicyError> {
        let capacity = self
            .critical_capacity
            .saturating_add(self.interactive_capacity)
            .saturating_add(self.normal_capacity)
            .saturating_add(self.bulk_capacity)
            .saturating_add(self.background_capacity)
            .saturating_add(self.maintenance_capacity);
        if self.max_queued == 0
            || self.max_queued_per_tenant == 0
            || self.max_queued_per_tenant > self.max_queued
            || self.max_queued_bytes == 0
            || capacity < self.max_queued
            || self.critical_capacity == 0
            || self.interactive_capacity == 0
            || self.normal_capacity == 0
            || self.bulk_capacity == 0
            || self.background_capacity == 0
            || self.maintenance_capacity == 0
            || self.critical_weight == 0
            || self.interactive_weight == 0
            || self.normal_weight == 0
            || self.bulk_weight == 0
            || self.background_weight == 0
            || self.maintenance_weight == 0
            || self.aging_quantum_ms == 0
        {
            return Err(PolicyError::InvalidQueuePolicy);
        }
        Ok(())
    }

    const fn class_capacity(self, class: WorkClass) -> usize {
        match class {
            WorkClass::Critical => self.critical_capacity,
            WorkClass::Interactive => self.interactive_capacity,
            WorkClass::Normal => self.normal_capacity,
            WorkClass::Bulk => self.bulk_capacity,
            WorkClass::Background => self.background_capacity,
            WorkClass::Maintenance => self.maintenance_capacity,
        }
    }

    const fn class_weight(self, class: WorkClass) -> u8 {
        match class {
            WorkClass::Critical => self.critical_weight,
            WorkClass::Interactive => self.interactive_weight,
            WorkClass::Normal => self.normal_weight,
            WorkClass::Bulk => self.bulk_weight,
            WorkClass::Background => self.background_weight,
            WorkClass::Maintenance => self.maintenance_weight,
        }
    }
}

/// One caller-owned value plus payload-free scheduling metadata.
#[derive(Debug)]
pub struct QueuedWork<T> {
    pub descriptor: WorkDescriptor,
    pub value: T,
    pub enqueued_at_ms: u64,
    pub tenant_weight: u16,
}

/// Bounded weighted round-robin queue with class aging and tenant rotation.
pub struct FairQueue<T> {
    config: FairQueueConfig,
    queues: BTreeMap<WorkClass, VecDeque<QueuedWork<T>>>,
    tenant_depths: BTreeMap<TenantId, usize>,
    schedule: Vec<WorkClass>,
    schedule_cursor: usize,
    tenant_run: BTreeMap<WorkClass, (TenantId, u16)>,
    len: usize,
    bytes: u64,
}

impl<T> FairQueue<T> {
    /// Creates an empty bounded fair queue.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::InvalidQueuePolicy`] when configuration is unsafe.
    pub fn new(config: FairQueueConfig) -> Result<Self, PolicyError> {
        config.validate()?;
        let mut queues = BTreeMap::new();
        let mut schedule = Vec::new();
        for class in CLASSES {
            queues.insert(class, VecDeque::new());
            schedule.extend(std::iter::repeat_n(
                class,
                usize::from(config.class_weight(class)),
            ));
        }
        Ok(Self {
            config,
            queues,
            tenant_depths: BTreeMap::new(),
            schedule,
            schedule_cursor: 0,
            tenant_run: BTreeMap::new(),
            len: 0,
            bytes: 0,
        })
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub const fn queued_bytes(&self) -> u64 {
        self.bytes
    }

    #[must_use]
    pub fn tenant_count(&self) -> usize {
        self.tenant_depths.len()
    }

    /// Adds ephemeral scheduling state. The caller remains responsible for durable discovery.
    ///
    /// # Errors
    ///
    /// Returns a [`QueueRejection`] before allocation when the global, class, tenant, or byte
    /// ceiling has been reached.
    pub fn enqueue(
        &mut self,
        descriptor: WorkDescriptor,
        value: T,
        enqueued_at_ms: u64,
        tenant_weight: u16,
    ) -> Result<(), QueueRejection> {
        let bytes = descriptor.estimated_bytes();
        if self.len >= self.config.max_queued
            || self
                .bytes
                .checked_add(bytes)
                .is_none_or(|total| total > self.config.max_queued_bytes)
        {
            return Err(QueueRejection::GlobalFull);
        }
        let class_depth = self.queues.get(&descriptor.class).map_or(0, VecDeque::len);
        if class_depth >= self.config.class_capacity(descriptor.class) {
            return Err(QueueRejection::ClassFull);
        }
        let tenant_depth = self
            .tenant_depths
            .get(&descriptor.tenant_id)
            .copied()
            .unwrap_or(0);
        if tenant_depth >= self.config.max_queued_per_tenant {
            return Err(QueueRejection::TenantFull);
        }
        let tenant_id = descriptor.tenant_id;
        if let Some(queue) = self.queues.get_mut(&descriptor.class) {
            queue.push_back(QueuedWork {
                descriptor,
                value,
                enqueued_at_ms,
                tenant_weight: tenant_weight.max(1),
            });
        }
        self.tenant_depths.insert(tenant_id, tenant_depth + 1);
        self.len += 1;
        self.bytes = self.bytes.saturating_add(bytes);
        Ok(())
    }

    /// Selects one item with weighted class rotation; sufficiently old heads bypass the schedule.
    pub fn dequeue(&mut self, now_ms: u64) -> Option<QueuedWork<T>> {
        self.dispatch(now_ms, |_| Some(())).map(|(item, ())| item)
    }

    /// Removes work only after the caller reserves its execution capacity.
    ///
    /// A failed reservation keeps the item and its queue accounting intact. Each class is
    /// visited once per call, with at most one candidate per tenant in that class. The host
    /// retains responsibility for discovering durable work.
    pub fn dispatch<P>(
        &mut self,
        now_ms: u64,
        mut reserve: impl FnMut(&QueuedWork<T>) -> Option<P>,
    ) -> Option<(QueuedWork<T>, P)> {
        let mut skipped = [false; CLASSES.len()];
        loop {
            let class = self
                .aged_class(now_ms, skipped)
                .or_else(|| self.scheduled_class(skipped))?;
            let preferred_index = self.tenant_fair_index(class);
            let queue = self.queues.get_mut(&class)?;
            let preferred = queue.get(preferred_index)?;
            let reservation = reserve(preferred)
                .map(|permit| (preferred_index, permit))
                .or_else(|| {
                    let mut tried = BTreeSet::from([preferred.descriptor.tenant_id]);
                    queue.iter().enumerate().find_map(|(index, item)| {
                        if tried.insert(item.descriptor.tenant_id) {
                            reserve(item).map(|permit| (index, permit))
                        } else {
                            None
                        }
                    })
                });
            if let Some((index, permit)) = reservation {
                let item = queue.remove(index)?;
                self.record_dequeue(class, &item);
                return Some((item, permit));
            }
            skipped[usize::from(class.rank())] = true;
        }
    }

    fn aged_class(&self, now_ms: u64, skipped: [bool; CLASSES.len()]) -> Option<WorkClass> {
        let starvation_age = self.config.aging_quantum_ms.saturating_mul(6);
        CLASSES
            .into_iter()
            .filter(|class| !skipped[usize::from(class.rank())])
            .filter_map(|class| {
                self.queues
                    .get(&class)
                    .and_then(|queue| queue.front())
                    .map(|head| (class, now_ms.saturating_sub(head.enqueued_at_ms)))
            })
            .filter(|(_, age)| *age >= starvation_age)
            .max_by_key(|(class, age)| (*age, std::cmp::Reverse(class.rank())))
            .map(|(class, _)| class)
    }

    fn scheduled_class(&mut self, skipped: [bool; CLASSES.len()]) -> Option<WorkClass> {
        for _ in 0..self.schedule.len() {
            let class = self.schedule[self.schedule_cursor];
            self.schedule_cursor = (self.schedule_cursor + 1) % self.schedule.len();
            if !skipped[usize::from(class.rank())]
                && self
                    .queues
                    .get(&class)
                    .is_some_and(|queue| !queue.is_empty())
            {
                return Some(class);
            }
        }
        None
    }

    fn tenant_fair_index(&self, class: WorkClass) -> usize {
        let Some(queue) = self.queues.get(&class) else {
            return 0;
        };
        let Some(front) = queue.front() else { return 0 };
        let Some((last_tenant, run)) = self.tenant_run.get(&class).copied() else {
            return 0;
        };
        if front.descriptor.tenant_id != last_tenant || run < front.tenant_weight {
            return 0;
        }
        queue
            .iter()
            .position(|item| item.descriptor.tenant_id != last_tenant)
            .unwrap_or(0)
    }

    fn record_dequeue(&mut self, class: WorkClass, item: &QueuedWork<T>) {
        let tenant_id = item.descriptor.tenant_id;
        let bytes = item.descriptor.estimated_bytes();
        let next_run = self
            .tenant_run
            .get(&class)
            .filter(|(previous, _)| *previous == tenant_id)
            .map_or(1, |(_, run)| run.saturating_add(1));
        self.tenant_run.insert(class, (tenant_id, next_run));
        if let Some(depth) = self.tenant_depths.get_mut(&tenant_id) {
            *depth = depth.saturating_sub(1);
            if *depth == 0 {
                self.tenant_depths.remove(&tenant_id);
            }
        }
        self.len = self.len.saturating_sub(1);
        self.bytes = self.bytes.saturating_sub(bytes);
    }
}

#[cfg(test)]
mod tests {
    use aequora_types::scheduler::WorkKind;

    use super::*;
    use crate::admission::{
        AdmissionPolicy, AdmissionRejection, CostUnits, HierarchicalAdmission, LoadState,
        OptionalWork, RequestShape, ResourceDomain,
    };

    fn work(tenant_id: TenantId, class: WorkClass) -> WorkDescriptor {
        WorkDescriptor {
            tenant_id,
            tenant_weight: 1,
            kind: WorkKind::PushOperations,
            class,
            cost: CostUnits::new(1),
            shape: RequestShape {
                decompressed_bytes: 1,
                ..RequestShape::default()
            },
            scope: None,
            estimated_response_bytes: 1,
            resources: vec![ResourceDomain::SyncExchange],
        }
    }

    #[test]
    fn dispatch_preserves_browned_out_work_and_admits_required_work() {
        let mut queue =
            FairQueue::new(FairQueueConfig::default()).unwrap_or_else(|error| panic!("{error}"));
        let controller = HierarchicalAdmission::new(AdmissionPolicy::default())
            .unwrap_or_else(|error| panic!("{error}"));
        let mut snapshot = work(TenantId::new(), WorkClass::Critical);
        snapshot.kind = WorkKind::LargeBootstrapTransfer;
        snapshot.resources = vec![ResourceDomain::SnapshotBuild];
        queue
            .enqueue(snapshot, Some(OptionalWork::BulkBuild), 0, 1)
            .unwrap_or_else(|error| panic!("{error:?}"));
        queue
            .enqueue(work(TenantId::new(), WorkClass::Interactive), None, 1, 1)
            .unwrap_or_else(|error| panic!("{error:?}"));

        controller.set_load_state(LoadState::Overloaded);
        let reserve = |item: &QueuedWork<Option<OptionalWork>>| match item.value {
            Some(optional) => controller.try_admit_optional(&item.descriptor, optional),
            None => controller.try_admit(&item.descriptor),
        };
        let initial_bytes = queue.queued_bytes();
        let (required, permit) = queue
            .dispatch(u64::MAX, |item| {
                let result = reserve(item);
                if item.value.is_some() {
                    assert!(matches!(result, Err(AdmissionRejection::Brownout { .. })));
                }
                result.ok()
            })
            .unwrap_or_else(|| panic!("required work should still dispatch"));
        assert_eq!(required.value, None);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.tenant_count(), 1);
        assert_eq!(
            queue.queued_bytes(),
            initial_bytes - required.descriptor.estimated_bytes()
        );
        assert_eq!(controller.snapshot().global_in_flight, 1);
        drop(permit);
        assert!(
            queue
                .dispatch(u64::MAX, |item| reserve(item).ok())
                .is_none()
        );
        assert_eq!(queue.len(), 1);

        controller.set_load_state(LoadState::Healthy);
        let (resumed, _permit) = queue
            .dispatch(u64::MAX, |item| reserve(item).ok())
            .unwrap_or_else(|| panic!("optional work should resume"));
        assert_eq!(resumed.value, Some(OptionalWork::BulkBuild));
        assert_eq!(resumed.enqueued_at_ms, 0);
        assert_eq!(queue.queued_bytes(), 0);
        assert_eq!(queue.tenant_count(), 0);
    }

    #[test]
    fn dispatch_tries_another_tenant_when_one_tenant_is_at_capacity() {
        let mut policy = AdmissionPolicy::default();
        policy.tenant.budget.max_in_flight = 1;
        let controller =
            HierarchicalAdmission::new(policy).unwrap_or_else(|error| panic!("{error}"));
        let mut queue =
            FairQueue::new(FairQueueConfig::default()).unwrap_or_else(|error| panic!("{error}"));
        let busy = TenantId::new();
        let quiet = TenantId::new();
        let existing = controller
            .try_admit(&work(busy, WorkClass::Normal))
            .unwrap_or_else(|error| panic!("{error}"));
        for (tenant, value) in [(busy, 1), (busy, 2), (quiet, 3)] {
            queue
                .enqueue(work(tenant, WorkClass::Normal), value, 0, 1)
                .unwrap_or_else(|error| panic!("{error:?}"));
        }
        let (next, _permit) = queue
            .dispatch(1, |item| controller.try_admit(&item.descriptor).ok())
            .unwrap_or_else(|| panic!("quiet tenant should dispatch"));
        assert_eq!(next.value, 3);
        assert_eq!(queue.len(), 2);
        drop(existing);
        let (next, _permit) = queue
            .dispatch(2, |item| controller.try_admit(&item.descriptor).ok())
            .unwrap_or_else(|| panic!("oldest work for the busy tenant should dispatch"));
        assert_eq!(next.value, 1);
    }

    #[test]
    fn queue_rejects_bytes_beyond_the_largest_representable_budget() {
        let mut queue = FairQueue::new(FairQueueConfig {
            max_queued_bytes: u64::MAX,
            ..FairQueueConfig::default()
        })
        .unwrap_or_else(|error| panic!("{error}"));
        let mut large = work(TenantId::new(), WorkClass::Bulk);
        large.estimated_response_bytes = u64::MAX - large.shape.decompressed_bytes;
        queue
            .enqueue(large, (), 0, 1)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(
            queue.enqueue(work(TenantId::new(), WorkClass::Bulk), (), 1, 1),
            Err(QueueRejection::GlobalFull)
        );
        assert_eq!(queue.queued_bytes(), u64::MAX);
        assert!(queue.dequeue(2).is_some());
        assert_eq!(queue.queued_bytes(), 0);
    }

    #[test]
    fn queue_is_globally_class_and_tenant_bounded() {
        let config = FairQueueConfig {
            max_queued: 2,
            max_queued_per_tenant: 1,
            ..FairQueueConfig::default()
        };
        let mut queue = FairQueue::new(config).unwrap_or_else(|error| panic!("{error}"));
        let tenant = TenantId::new();
        queue
            .enqueue(work(tenant, WorkClass::Normal), 1, 0, 1)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            queue.enqueue(work(tenant, WorkClass::Normal), 2, 0, 1),
            Err(QueueRejection::TenantFull)
        );
        queue
            .enqueue(work(TenantId::new(), WorkClass::Normal), 3, 0, 1)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            queue.enqueue(work(TenantId::new(), WorkClass::Normal), 4, 0, 1),
            Err(QueueRejection::GlobalFull)
        );
    }

    #[test]
    fn old_maintenance_progresses_despite_interactive_load() {
        let mut queue =
            FairQueue::new(FairQueueConfig::default()).unwrap_or_else(|error| panic!("{error}"));
        queue
            .enqueue(work(TenantId::new(), WorkClass::Maintenance), "old", 0, 1)
            .unwrap_or_else(|error| panic!("{error}"));
        queue
            .enqueue(
                work(TenantId::new(), WorkClass::Interactive),
                "new",
                6_000,
                1,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(queue.dequeue(6_000).map(|item| item.value), Some("old"));
    }

    #[test]
    fn noisy_tenant_cannot_hide_another_tenant_in_one_class() {
        let mut queue =
            FairQueue::new(FairQueueConfig::default()).unwrap_or_else(|error| panic!("{error}"));
        let noisy = TenantId::new();
        let small = TenantId::new();
        for value in 0..4 {
            queue
                .enqueue(work(noisy, WorkClass::Normal), value, 1, 1)
                .unwrap_or_else(|error| panic!("{error}"));
        }
        queue
            .enqueue(work(small, WorkClass::Normal), 99, 1, 1)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(queue.dequeue(1).map(|item| item.value), Some(0));
        assert_eq!(queue.dequeue(1).map(|item| item.value), Some(99));
    }
}
