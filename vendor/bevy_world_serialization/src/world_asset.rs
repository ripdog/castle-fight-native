use core::{alloc::Layout, any::TypeId, ptr::NonNull};

use alloc::alloc::dealloc;

use crate::reflect_utils::clone_reflect_value;
use crate::{DynamicWorld, WorldInstanceSpawnError};
use bevy_asset::Asset;
use bevy_ecs::resource::IS_RESOURCE;
use bevy_ecs::{
    component::{ComponentCloneBehavior, ComponentId},
    entity::{Entity, EntityHashMap, SceneEntityMapper},
    entity_disabling::DefaultQueryFilters,
    reflect::{AppTypeRegistry, ReflectComponent, ReflectResource},
    relationship::RelationshipHookMode,
    ptr::OwningPtr,
    world::{EntityWorldMut, World},
};
use bevy_reflect::{Reflect, TypePath, TypeRegistry};

/// A composition of [`World`] objects.
///
/// To spawn a [`WorldAsset`], you can use either:
/// * [`WorldInstanceSpawner::spawn`](crate::WorldInstanceSpawner::spawn)
/// * adding the [`WorldAssetRoot`](crate::components::WorldAssetRoot) component to an entity.
#[derive(Asset, TypePath, Debug)]
pub struct WorldAsset {
    /// The world, containing its entities and resources.
    pub world: World,
}

impl WorldAsset {
    /// Creates a new [`WorldAsset`] with the given world.
    pub fn new(world: World) -> Self {
        Self { world }
    }

    /// Create a new [`WorldAsset`] from a given dynamic world.
    pub fn from_dynamic_world(
        dynamic_world: &DynamicWorld,
        type_registry: &TypeRegistry,
    ) -> Result<WorldAsset, WorldInstanceSpawnError> {
        let mut world = World::new();
        let mut entity_map = EntityHashMap::default();
        dynamic_world.write_to_world_with(&mut world, &mut entity_map, type_registry)?;

        Ok(Self { world })
    }

    /// Clone the [`WorldAsset`].
    ///
    /// This method will return a [`WorldInstanceSpawnError`] if a type either is not registered in the
    /// provided [`AppTypeRegistry`] or doesn't reflect the [`Component`](bevy_ecs::component::Component) trait.
    pub fn clone_with(
        &self,
        type_registry: &AppTypeRegistry,
    ) -> Result<WorldAsset, WorldInstanceSpawnError> {
        let mut new_world = World::new();
        let mut entity_map = EntityHashMap::default();
        self.write_to_world_with(&mut new_world, &mut entity_map, type_registry)?;
        Ok(Self { world: new_world })
    }

    /// Write the entities and their corresponding components to the given world.
    ///
    /// This method will return a [`WorldInstanceSpawnError`] if a type either is not registered in the
    /// provided [`AppTypeRegistry`] or doesn't reflect the [`Component`](bevy_ecs::component::Component) trait.
    pub fn write_to_world_with(
        &self,
        world: &mut World,
        entity_map: &mut EntityHashMap<Entity>,
        type_registry: &AppTypeRegistry,
    ) -> Result<(), WorldInstanceSpawnError> {
        let type_registry = type_registry.read();

        let self_dqf_id = self
            .world
            .components()
            .get_id(TypeId::of::<DefaultQueryFilters>());

        // Resources archetype
        for (component_id, source_entity) in self.world.resource_entities().iter() {
            if Some(component_id) == self_dqf_id {
                continue;
            }
            if !world
                .get_entity(source_entity)
                .ok()
                .is_some_and(|entity_ref| entity_ref.contains_id(component_id))
            {
                continue;
            }

            let component_info = self
                .world
                .components()
                .get_info(component_id)
                .expect("component_ids in archetypes should have ComponentInfo");

            let type_id = component_info
                .type_id()
                .expect("reflected resources must have a type_id");

            let registration = type_registry.get(type_id).ok_or_else(|| {
                WorldInstanceSpawnError::UnregisteredType {
                    std_type_name: component_info.name(),
                }
            })?;
            registration.data::<ReflectResource>().ok_or_else(|| {
                WorldInstanceSpawnError::UnregisteredResource {
                    type_path: registration.type_info().type_path().to_string(),
                }
            })?;
            // reflect_resource existing, implies that reflect_component also exists
            let reflect_component = registration
                .data::<ReflectComponent>()
                .expect("ReflectComponent is depended on ReflectResource");

            // check if the resource already exists in the other world, if not spawn it
            let destination_entity =
                if let Some(entity) = world.resource_entities().get(component_id) {
                    entity
                } else {
                    world.spawn_empty().id()
                };

            reflect_component.copy(
                &self.world,
                world,
                source_entity,
                destination_entity,
                &type_registry,
            );
        }

        // Ensure that all source world entities have been allocated in the destination
        // world before handling components that may contain references that need mapping.
        for archetype in self.world.archetypes().iter() {
            if archetype.contains(IS_RESOURCE) {
                continue;
            }
            for scene_entity in archetype.entities() {
                entity_map
                    .entry(scene_entity.id())
                    .or_insert_with(|| world.spawn_empty().id());
            }
        }

        // Resolve each archetype's components once, then clone an entity's components into a
        // buffer and insert them together: one archetype move per entity rather than one per
        // component. Relationships keep the per-component path, which skips their hooks.
        let mut plan = Vec::new();
        let mut batch = ReflectedBatch::default();
        let mut deferred = Vec::new();
        for archetype in self.world.archetypes().iter() {
            if archetype.contains(IS_RESOURCE) {
                continue;
            }
            plan.clear();
            for component_id in archetype.iter_components() {
                let component_info = self
                    .world
                    .components()
                    .get_info(component_id)
                    .expect("component_ids in archetypes should have ComponentInfo");

                if matches!(
                    *component_info.clone_behavior(),
                    ComponentCloneBehavior::Ignore
                ) {
                    continue;
                }

                let type_id = component_info.type_id().unwrap();
                let registration = type_registry.get(type_id).ok_or_else(|| {
                    WorldInstanceSpawnError::UnregisteredType {
                        std_type_name: component_info.name(),
                    }
                })?;
                let reflect_component =
                    registration.data::<ReflectComponent>().ok_or_else(|| {
                        WorldInstanceSpawnError::UnregisteredComponent {
                            type_path: registration.type_info().type_path().to_string(),
                        }
                    })?;
                let batched = component_info
                    .relationship_accessor()
                    .is_none()
                    .then(|| (reflect_component.register_component(world), type_id));
                plan.push((registration, reflect_component, batched));
            }

            for scene_entity in archetype.entities() {
                let entity = *entity_map
                    .get(&scene_entity.id())
                    .expect("should have previously spawned an entity");
                let source = self.world.entity(scene_entity.id());

                // If a component references entities in the source world, update them to the
                // entities in the destination world.
                SceneEntityMapper::world_scope(entity_map, world, |world, mapper| {
                    for &(registration, reflect_component, batched) in &plan {
                        let Some(component) = reflect_component.reflect(source) else {
                            continue;
                        };
                        if let Some((component_id, type_id)) = batched
                            && !world.entity(entity).contains_id(component_id)
                            && let Ok(mut value) = component.reflect_clone()
                            && (*value).as_any().type_id() == type_id
                        {
                            reflect_component.map_entities(&mut *value, mapper);
                            batch.push(component_id, value);
                        } else {
                            deferred.push((
                                reflect_component,
                                clone_reflect_value(component.as_partial_reflect(), registration),
                            ));
                        }
                    }
                    let mut entity_mut = world.entity_mut(entity);
                    batch.insert(&mut entity_mut);
                    for (reflect_component, component) in deferred.drain(..) {
                        reflect_component.apply_or_insert_mapped(
                            &mut entity_mut,
                            component.as_partial_reflect(),
                            &type_registry,
                            mapper,
                            RelationshipHookMode::Skip,
                        );
                    }
                });
            }
        }

        Ok(())
    }
}

/// Reflected component values waiting to be inserted into one entity together.
#[derive(Default)]
struct ReflectedBatch {
    ids: Vec<ComponentId>,
    values: Vec<(NonNull<u8>, Layout)>,
}

impl ReflectedBatch {
    /// Queues `value`, whose concrete type must be the component that `id` names.
    fn push(&mut self, id: ComponentId, value: Box<dyn Reflect>) {
        let layout = Layout::for_value(&*value);
        let value = NonNull::from(Box::leak(value)).cast::<u8>();
        self.ids.push(id);
        self.values.push((value, layout));
    }

    /// Moves the queued values into `entity` with one insert and frees their boxes.
    #[expect(
        unsafe_code,
        reason = "inserting several dynamically typed components at once needs `insert_by_ids`"
    )]
    fn insert(&mut self, entity: &mut EntityWorldMut) {
        if self.ids.is_empty() {
            return;
        }
        // SAFETY: each pointer is a leaked `Box` holding an initialized value whose type id was
        // checked against the component that the matching entry of `ids` names, registered in
        // this world, so it is valid, aligned and owned here. `insert_by_ids` moves every value
        // out and nothing reads the boxes afterwards.
        unsafe {
            entity.insert_by_ids(
                &self.ids,
                self.values
                    .iter()
                    .map(|&(value, _)| OwningPtr::new(value)),
            );
        }
        for (value, layout) in self.values.drain(..) {
            if layout.size() != 0 {
                // SAFETY: the pointer came from a `Box` allocated with this layout, and its
                // value was moved out above, so it is freed without being dropped.
                unsafe { dealloc(value.as_ptr(), layout) };
            }
        }
        self.ids.clear();
    }
}
