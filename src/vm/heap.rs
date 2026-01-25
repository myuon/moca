use std::collections::HashMap;
use std::fmt;

use super::Value;

/// Type ID for heap objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    String,
    Array,
    Object,
}

/// Header for all heap objects.
#[derive(Debug)]
pub struct ObjectHeader {
    pub obj_type: ObjectType,
    pub marked: bool,
}

/// A heap-allocated string.
#[derive(Debug)]
pub struct MicaString {
    pub header: ObjectHeader,
    pub value: String,
}

/// A heap-allocated array.
#[derive(Debug)]
pub struct MicaArray {
    pub header: ObjectHeader,
    pub elements: Vec<Value>,
}

/// A heap-allocated object (key-value map).
#[derive(Debug)]
pub struct MicaObject {
    pub header: ObjectHeader,
    pub fields: HashMap<String, Value>,
    /// Shape ID for inline caching (objects with same fields have same shape)
    pub shape_id: u32,
}

/// A heap object can be any of the heap-allocated types.
pub enum HeapObject {
    String(MicaString),
    Array(MicaArray),
    Object(MicaObject),
}

impl HeapObject {
    pub fn obj_type(&self) -> ObjectType {
        match self {
            HeapObject::String(_) => ObjectType::String,
            HeapObject::Array(_) => ObjectType::Array,
            HeapObject::Object(_) => ObjectType::Object,
        }
    }

    pub fn header(&self) -> &ObjectHeader {
        match self {
            HeapObject::String(s) => &s.header,
            HeapObject::Array(a) => &a.header,
            HeapObject::Object(o) => &o.header,
        }
    }

    pub fn header_mut(&mut self) -> &mut ObjectHeader {
        match self {
            HeapObject::String(s) => &mut s.header,
            HeapObject::Array(a) => &mut a.header,
            HeapObject::Object(o) => &mut o.header,
        }
    }

    pub fn as_string(&self) -> Option<&MicaString> {
        match self {
            HeapObject::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&MicaArray> {
        match self {
            HeapObject::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut MicaArray> {
        match self {
            HeapObject::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&MicaObject> {
        match self {
            HeapObject::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut MicaObject> {
        match self {
            HeapObject::Object(o) => Some(o),
            _ => None,
        }
    }

    /// Get all Value references in this object for GC tracing.
    pub fn trace(&self) -> Vec<GcRef> {
        match self {
            HeapObject::String(_) => vec![],
            HeapObject::Array(arr) => arr
                .elements
                .iter()
                .filter_map(|v| v.as_ptr())
                .collect(),
            HeapObject::Object(obj) => obj
                .fields
                .values()
                .filter_map(|v| v.as_ptr())
                .collect(),
        }
    }
}

impl fmt::Debug for HeapObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeapObject::String(s) => write!(f, "String({:?})", s.value),
            HeapObject::Array(a) => write!(f, "Array({:?})", a.elements),
            HeapObject::Object(o) => write!(f, "Object({:?})", o.fields),
        }
    }
}

/// A reference to a heap object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcRef {
    pub index: usize,
}

/// The garbage-collected heap.
pub struct Heap {
    objects: Vec<Option<HeapObject>>,
    free_list: Vec<usize>,
    bytes_allocated: usize,
    gc_threshold: usize,
    /// Counter for generating unique shape IDs
    next_shape_id: u32,
    /// Cache of shape signatures to shape IDs
    shape_cache: HashMap<Vec<String>, u32>,
}

impl Heap {
    pub fn new() -> Self {
        Self {
            objects: Vec::new(),
            free_list: Vec::new(),
            bytes_allocated: 0,
            gc_threshold: 1024 * 1024, // 1MB initial threshold
            next_shape_id: 1,
            shape_cache: HashMap::new(),
        }
    }

    /// Compute or retrieve a shape ID for a given set of field names.
    fn get_shape_id(&mut self, field_names: &[String]) -> u32 {
        let mut sorted_names: Vec<String> = field_names.to_vec();
        sorted_names.sort();

        if let Some(&id) = self.shape_cache.get(&sorted_names) {
            id
        } else {
            let id = self.next_shape_id;
            self.next_shape_id += 1;
            self.shape_cache.insert(sorted_names, id);
            id
        }
    }

    /// Allocate a new string on the heap.
    pub fn alloc_string(&mut self, value: String) -> GcRef {
        let size = std::mem::size_of::<MicaString>() + value.len();
        self.bytes_allocated += size;

        let obj = HeapObject::String(MicaString {
            header: ObjectHeader {
                obj_type: ObjectType::String,
                marked: false,
            },
            value,
        });
        self.alloc_object(obj)
    }

    /// Allocate a new array on the heap.
    pub fn alloc_array(&mut self, elements: Vec<Value>) -> GcRef {
        let size = std::mem::size_of::<MicaArray>() + elements.len() * std::mem::size_of::<Value>();
        self.bytes_allocated += size;

        let obj = HeapObject::Array(MicaArray {
            header: ObjectHeader {
                obj_type: ObjectType::Array,
                marked: false,
            },
            elements,
        });
        self.alloc_object(obj)
    }

    /// Allocate a new object on the heap.
    pub fn alloc_object_map(&mut self, fields: HashMap<String, Value>) -> GcRef {
        let size = std::mem::size_of::<MicaObject>()
            + fields.len() * (std::mem::size_of::<String>() + std::mem::size_of::<Value>());
        self.bytes_allocated += size;

        // Compute shape ID based on field names
        let field_names: Vec<String> = fields.keys().cloned().collect();
        let shape_id = self.get_shape_id(&field_names);

        let obj = HeapObject::Object(MicaObject {
            header: ObjectHeader {
                obj_type: ObjectType::Object,
                marked: false,
            },
            fields,
            shape_id,
        });
        self.alloc_object(obj)
    }

    fn alloc_object(&mut self, obj: HeapObject) -> GcRef {
        if let Some(index) = self.free_list.pop() {
            self.objects[index] = Some(obj);
            GcRef { index }
        } else {
            let index = self.objects.len();
            self.objects.push(Some(obj));
            GcRef { index }
        }
    }

    /// Get an object by reference.
    pub fn get(&self, r: GcRef) -> Option<&HeapObject> {
        self.objects.get(r.index).and_then(|o| o.as_ref())
    }

    /// Get a mutable reference to an object.
    pub fn get_mut(&mut self, r: GcRef) -> Option<&mut HeapObject> {
        self.objects.get_mut(r.index).and_then(|o| o.as_mut())
    }

    /// Check if GC should be triggered.
    pub fn should_gc(&self) -> bool {
        self.bytes_allocated >= self.gc_threshold
    }

    /// Get the number of bytes currently allocated.
    pub fn bytes_allocated(&self) -> usize {
        self.bytes_allocated
    }

    /// Mark phase: mark all reachable objects.
    pub fn mark(&mut self, roots: &[Value]) {
        // Collect all root references
        let mut worklist: Vec<GcRef> = roots.iter().filter_map(|v| v.as_ptr()).collect();

        // Mark and trace
        while let Some(r) = worklist.pop() {
            if let Some(obj) = self.objects.get_mut(r.index).and_then(|o| o.as_mut()) {
                if !obj.header().marked {
                    obj.header_mut().marked = true;
                    // Trace children
                    worklist.extend(obj.trace());
                }
            }
        }
    }

    /// Sweep phase: free all unmarked objects.
    pub fn sweep(&mut self) {
        for i in 0..self.objects.len() {
            if let Some(obj) = &mut self.objects[i] {
                if obj.header().marked {
                    // Reset mark for next GC cycle
                    obj.header_mut().marked = false;
                } else {
                    // Free unmarked object
                    self.objects[i] = None;
                    self.free_list.push(i);
                }
            }
        }

        // Recalculate bytes allocated
        self.bytes_allocated = 0;
        for obj in &self.objects {
            if let Some(o) = obj {
                self.bytes_allocated += match o {
                    HeapObject::String(s) => {
                        std::mem::size_of::<MicaString>() + s.value.len()
                    }
                    HeapObject::Array(a) => {
                        std::mem::size_of::<MicaArray>()
                            + a.elements.len() * std::mem::size_of::<Value>()
                    }
                    HeapObject::Object(o) => {
                        std::mem::size_of::<MicaObject>()
                            + o.fields.len()
                                * (std::mem::size_of::<String>() + std::mem::size_of::<Value>())
                    }
                };
            }
        }

        // Adjust threshold
        self.gc_threshold = (self.bytes_allocated * 2).max(1024 * 1024);
    }

    /// Perform a full garbage collection cycle.
    pub fn collect(&mut self, roots: &[Value]) {
        self.mark(roots);
        self.sweep();
    }

    /// Get count of live objects.
    pub fn object_count(&self) -> usize {
        self.objects.iter().filter(|o| o.is_some()).count()
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_string() {
        let mut heap = Heap::new();
        let r = heap.alloc_string("hello".to_string());
        let obj = heap.get(r).unwrap();
        assert_eq!(obj.as_string().unwrap().value, "hello");
    }

    #[test]
    fn test_alloc_array() {
        let mut heap = Heap::new();
        let r = heap.alloc_array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        let obj = heap.get(r).unwrap();
        assert_eq!(obj.as_array().unwrap().elements.len(), 3);
    }

    #[test]
    fn test_gc_collects_unreachable() {
        let mut heap = Heap::new();

        // Allocate some objects
        let _r1 = heap.alloc_string("garbage".to_string());
        let r2 = heap.alloc_string("keep".to_string());

        assert_eq!(heap.object_count(), 2);

        // Only r2 is in roots
        heap.collect(&[Value::Ptr(r2)]);

        assert_eq!(heap.object_count(), 1);
        assert!(heap.get(r2).is_some());
    }

    #[test]
    fn test_gc_traces_arrays() {
        let mut heap = Heap::new();

        // Create a string
        let str_ref = heap.alloc_string("inside array".to_string());

        // Create an array containing the string
        let arr_ref = heap.alloc_array(vec![Value::Ptr(str_ref)]);

        assert_eq!(heap.object_count(), 2);

        // Only array is in roots, but string should be kept via tracing
        heap.collect(&[Value::Ptr(arr_ref)]);

        assert_eq!(heap.object_count(), 2);
        assert!(heap.get(str_ref).is_some());
        assert!(heap.get(arr_ref).is_some());
    }
}
