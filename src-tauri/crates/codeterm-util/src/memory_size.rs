use serde::ser::{self, Serialize};

pub fn estimate(value: &impl Serialize) -> usize {
    let mut size = Size(0);
    let _ = value.serialize(&mut size);
    size.0
}

struct Size(usize);

impl Size {
    fn add(&mut self, bytes: usize) {
        self.0 = self.0.saturating_add(bytes);
    }
}

impl ser::Serializer for &mut Size {
    type Ok = ();
    type Error = serde_json::Error;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, _: bool) -> Result<(), Self::Error> {
        self.add(1);
        Ok(())
    }

    fn serialize_i8(self, _: i8) -> Result<(), Self::Error> {
        self.add(1);
        Ok(())
    }

    fn serialize_i16(self, _: i16) -> Result<(), Self::Error> {
        self.add(2);
        Ok(())
    }

    fn serialize_i32(self, _: i32) -> Result<(), Self::Error> {
        self.add(4);
        Ok(())
    }

    fn serialize_i64(self, _: i64) -> Result<(), Self::Error> {
        self.add(8);
        Ok(())
    }

    fn serialize_i128(self, _: i128) -> Result<(), Self::Error> {
        self.add(16);
        Ok(())
    }

    fn serialize_u8(self, _: u8) -> Result<(), Self::Error> {
        self.add(1);
        Ok(())
    }

    fn serialize_u16(self, _: u16) -> Result<(), Self::Error> {
        self.add(2);
        Ok(())
    }

    fn serialize_u32(self, _: u32) -> Result<(), Self::Error> {
        self.add(4);
        Ok(())
    }

    fn serialize_u64(self, _: u64) -> Result<(), Self::Error> {
        self.add(8);
        Ok(())
    }

    fn serialize_u128(self, _: u128) -> Result<(), Self::Error> {
        self.add(16);
        Ok(())
    }

    fn serialize_f32(self, _: f32) -> Result<(), Self::Error> {
        self.add(4);
        Ok(())
    }

    fn serialize_f64(self, _: f64) -> Result<(), Self::Error> {
        self.add(8);
        Ok(())
    }

    fn serialize_char(self, _: char) -> Result<(), Self::Error> {
        self.add(4);
        Ok(())
    }

    fn serialize_str(self, value: &str) -> Result<(), Self::Error> {
        self.add(std::mem::size_of::<String>() + value.len());
        Ok(())
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<(), Self::Error> {
        self.add(std::mem::size_of::<Vec<u8>>() + value.len());
        Ok(())
    }

    fn serialize_none(self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), Self::Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<(), Self::Error> {
        self.add(4);
        Ok(())
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(self)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self, Self::Error> {
        self.add(std::mem::size_of::<Vec<()>>());
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, Self::Error> {
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, Self::Error> {
        Ok(self)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, Self::Error> {
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self, Self::Error> {
        self.add(std::mem::size_of::<Vec<()>>());
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, Self::Error> {
        Ok(self)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, Self::Error> {
        Ok(self)
    }
}

impl ser::SerializeSeq for &mut Size {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ser::SerializeTuple for &mut Size {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ser::SerializeTupleStruct for &mut Size {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ser::SerializeTupleVariant for &mut Size {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ser::SerializeStruct for &mut Size {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        _: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ser::SerializeStructVariant for &mut Size {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        _: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ser::SerializeMap for &mut Size {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut **self)
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
