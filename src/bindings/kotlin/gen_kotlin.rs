/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::{
    env,
    collections::HashMap,
    convert::TryFrom, convert::TryInto,
    fs::File,
    iter::IntoIterator,
    fmt::Display,
    path::{Path, PathBuf},
};

use anyhow::bail;
use anyhow::Result;
use askama::Template;

use crate::interface::*;

// Some config options for it the caller wants to customize the generated Kotlin.
// Note that this can only be used to control details of the Kotlin *that do not affect the underlying component*,
// sine the details of the underlying component are entirely determined by the `ComponentInterface`.
pub struct Config {
    pub package_name: String
}

impl Config {
    pub fn from(ci: &ComponentInterface) -> Self {
        Config {
            package_name: format!("uniffi.{}", ci.namespace())
        }
    }
}

#[derive(Template)]
#[template(ext="kt", escape="none", source=r#"
// This file was autogenerated by some hot garbage in the `uniffi` crate.
// Trust me, you don't want to mess with it!

package {{ config.package_name }};

// Common helper code.
//
// Ideally this would live in a separate .kt file where it can be unittested etc
// in isolation, and perhaps even published as a re-useable package.
//
// However, it's important that the detils of how this helper code works (e.g. the
// way that different builtin types are passed across the FFI) exactly match what's
// expected by the rust code on the other side of the interface. In practice right
// now that means come from the exact some version of `uniffi` that was used to
// compile the rust component. The easiest way to ensure this is to bundle the Kotlin
// helpers directly inline.

import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer
import com.sun.jna.Structure
import java.nio.ByteBuffer
import java.nio.ByteOrder

inline fun <reified Lib : Library> loadIndirect(
    componentName: String
): Lib {
    // XXX TODO: This will probably grow some magic for resolving megazording in future.
    // E.g. we might start by looking for the named component in `libuniffi.so` and if
    // that fails, fall back to loading it separately from `lib${componentName}.so`.
    return Native.load<Lib>("uniffi_${componentName}", Lib::class.java)
}

@Structure.FieldOrder("len", "data")
open class RustBuffer : Structure() {
    @JvmField var len: Long = 0
    @JvmField var data: Pointer? = null

    class ByValue : RustBuffer(), Structure.ByValue

    @Suppress("TooGenericExceptionThrown")
    fun asByteBuffer(): ByteBuffer? {
        return this.data?.let {
            val buf = it.getByteBuffer(0, this.len)
            buf.order(ByteOrder.BIG_ENDIAN)
            return buf
        }
    }
}

public fun Boolean.Companion.deserializeItemFromRust(buf: ByteBuffer): Boolean {
    return buf.get().toInt() != 0
}

public fun Byte.Companion.deserializeItemFromRust(buf: ByteBuffer): Byte {
    return buf.get()
}

public fun Int.Companion.deserializeItemFromRust(buf: ByteBuffer): Int {
    return buf.getInt()
}

public fun Int.serializeForRustSize(): Int {
    return 4
}

public fun Int.serializeForRustInto(buf: ByteBuffer) {
    buf.putInt(this)
}

public fun Float.Companion.deserializeItemFromRust(buf: ByteBuffer): Float {
    return buf.getFloat()
}

public fun Float.serializeForRustSize(): Int {
    return 4
}

public fun Float.serializeForRustInto(buf: ByteBuffer) {
    buf.putFloat(this)
}

public fun Double.Companion.deserializeItemFromRust(buf: ByteBuffer): Double {
    return buf.getDouble()
}

public fun Double.serializeForRustSize(): Int {
    return 8
}

public fun Double.serializeForRustInto(buf: ByteBuffer) {
    buf.putDouble(this)
}

public fun<T> T?.serializeForRustSize(): Int {
    if (this === null) return 1
    return 1 + this.serializeForRustSize()
}

public fun<T> T?.serializeForRustInto(buf: ByteBuffer) {
    if (this === null) buf.put(0)
    else {
        buf.put(1)
        this.serializeForRustInto(buf)
    }
}

internal fun Any?.serializeForRust(): RustBuffer.ByValue {
    val buf = _UniFFILib.INSTANCE.{{ ci.ffi_bytebuffer_alloc().name() }}(this.serializeForRustSize())
    try {
        this.serializeForRustInto(buf.asByteBuffer()!!)
        return buf
    } catch (e: Throwable) {
        _UniFFILib.INSTANCE.{{ ci.ffi_bytebuffer_free().name() }}(buf)
        throw e;
    }
}

public fun<T> deserializeFromRust(rbuf: RustBuffer.ByValue, deserializeItemFromRust: (ByteBuffer) -> T): T {
    val buf = rbuf.asByteBuffer()!!
    try {
       val item = deserializeItemFromRust(buf)
       if (buf.hasRemaining()) {
           throw RuntimeException("junk remaining in record buffer, something is very wrong!!")
       }
       return item
    } finally {
        _UniFFILib.INSTANCE.{{ ci.ffi_bytebuffer_free().name() }}(rbuf)
    }
}

// A JNA Library to expose the extern-C FFI definitions.
// This is an implementation detail which will be called internally by the public API.

internal interface _UniFFILib : Library {
    companion object {
        internal var INSTANCE: _UniFFILib = loadIndirect(componentName = "{{ ci.namespace() }}")
    }

    {% for func in ci.iter_ffi_function_definitions() -%}
        fun {{ func.name() }}(
        {%- for arg in func.arguments() %}
            {{ arg.name() }}: {{ arg.type_()|decl_c_argument }}{% if loop.last %}{% else %},{% endif %}
        {%- endfor %}
        // TODO: When we implement error handling, there will be an out error param here.
        ) {%- match func.return_type() -%}
        {%- when Some with (type_) %}
            : {{ type_|decl_c_return }}
        {% when None -%}
        {%- endmatch %}
    {% endfor -%}
}

// Public interface members begin here.

{% for e in ci.iter_enum_definitions() %}
    enum class {{ e.name() }} {
        {% for value in e.values() %}
        {{ value }}{% if loop.last %};{% else %},{% endif %}
        {% endfor %}

        companion object {
            internal fun fromOrdinal(n: Int): {{ e.name() }} {
                return when (n) {
                  {% for value in e.values() %}
                  {{ loop.index }} -> {{ value }}
                  {% endfor %}
                  else -> {
                      throw RuntimeException("invalid enum value, something is very wrong!!")
                  }
                }
            }
        }
    }
{%- endfor -%}

{%- for rec in ci.iter_record_definitions() %}
    data class {{ rec.name() }} (
      {%- for field in rec.fields() %}
        val {{ field.name() }}: {{ field.type_()|decl_kt }}{% if loop.last %}{% else %},{% endif %}
      {%- endfor %}
    ) {
      companion object {
          internal fun deserializeItemFromRust(buf: ByteBuffer): {{ rec.name() }} {
              return {{ rec.name() }}(
                {%- for field in rec.fields() %}
                {{ "buf"|deserialize_item_kt(field.type_()) }}{% if loop.last %}{% else %},{% endif %}
                {%- endfor %}
              )
          }
      }

      internal fun serializeForRust(): RustBuffer.ByValue {
          val buf = _UniFFILib.INSTANCE.{{ ci.ffi_bytebuffer_alloc().name() }}(this.serializeForRustSize())
          try {
                this.serializeForRustInto(buf.asByteBuffer()!!)
                return buf
          } catch (e: Throwable) {
                _UniFFILib.INSTANCE.{{ ci.ffi_bytebuffer_free().name() }}(buf)
                throw e;
          }
      }

      internal fun serializeForRustSize(): Int {
          return 0 +
            {%- for field in rec.fields() %}
            this.{{ field.name() }}.serializeForRustSize(){% if loop.last %}{% else %} +{% endif %}
            {%- endfor %}
      }

      internal fun serializeForRustInto(buf: ByteBuffer) {
          {%- for field in rec.fields() %}
          this.{{ field.name() }}.serializeForRustInto(buf)
          {%- endfor %}
      }
    }

{% endfor %}

{% for func in ci.iter_function_definitions() %}

    {%- match func.return_type() -%}
    {%- when Some with (return_type) %}

        fun {{ func.name() }}(
            {%- for arg in func.arguments() %}
                {{ arg.name() }}: {{ arg.type_()|decl_kt }}{% if loop.last %}{% else %},{% endif %}
            {%- endfor %}
        ): {{ return_type|decl_kt }} {
            val _retval = _UniFFILib.INSTANCE.{{ func.ffi_func().name() }}(
                {%- for arg in func.arguments() %}
                    {{ arg.name()|lower_kt(arg.type_()) }}{% if loop.last %}{% else %},{% endif %}
                    {%- endfor %}
            )
            return {{ "_retval"|lift_kt(return_type) }}
        }

    {% when None -%}

        fun {{ func.name() }}(
            {%- for arg in func.arguments() %}
                {{ arg.name() }}: {{ arg.type_()|decl_kt }}{% if loop.last %}{% else %},{% endif %}
            {%- endfor %}
        ) {
            UniFFILib.INSTANCE.{{ func.ffi_func().name() }}(
                {%- for arg in func.arguments() %}
                    {{ arg.name()|lower_kt(arg.type_()) }}{% if loop.last %}{% else %},{% endif %}
                    {%- endfor %}
            )
        }

    {%- endmatch %}
{% endfor %}

{% for obj in ci.iter_object_definitions() %}
 // TODO: object ({{ "{:?}"|format(obj)}})
{% endfor %}
"#)]
pub struct KotlinWrapper<'a> {
    config: Config,
    ci: &'a ComponentInterface,
}
impl<'a> KotlinWrapper<'a> {
    pub fn new(config: Config, ci: &'a ComponentInterface) -> Self {
        Self { config, ci }
    }
}

mod filters {
    use std::fmt;
    use super::*;

    pub fn decl_c_argument(type_: &TypeReference) -> Result<String, askama::Error> {
        Ok(match type_ {
            TypeReference::Boolean => "Byte".to_string(),
            TypeReference::U32 => "Int".to_string(),
            TypeReference::U64 => "Long".to_string(),
            TypeReference::Float => "Float".to_string(),
            TypeReference::Double => "Double".to_string(),
            TypeReference::String => "String".to_string(),
            TypeReference::Bytes => "RustBuffer.ByValue".to_string(),
            TypeReference::Enum(_) => "Int".to_string(),
            TypeReference::Record(_) => "RustBuffer.ByValue".to_string(),
            TypeReference::Optional(_) => "RustBuffer.ByValue".to_string(),
            _ => panic!("[TODO: decl_c_argument({:?})]", type_),
        })
    }

    pub fn decl_c_return(type_: &TypeReference) -> Result<String, askama::Error> {
        Ok(match type_ {
            TypeReference::String => "String".to_string(), // XXX TODO: I think maybe needs to be a ByteBuffer in return position..?
            _ => decl_c_argument(type_)?
        })
    }

    pub fn decl_kt(type_: &TypeReference) -> Result<String, askama::Error> {
        Ok(match type_ {
            TypeReference::Boolean => "Boolean".to_string(),
            TypeReference::Enum(name) => name.clone(),
            TypeReference::Record(name) => name.clone(),
            TypeReference::Optional(t) => format!("{}?", decl_kt(t)?),
            _ => decl_c_argument(type_)?
        })
    }

    pub fn lower_kt(nm: &dyn fmt::Display, type_: &TypeReference) -> Result<String, askama::Error> {
        let nm = nm.to_string();
        Ok(match type_ {
            TypeReference::Boolean => format!("(if ({}) {{ 1 }} else {{ 0 }})", nm),
            TypeReference::U32 => nm,
            TypeReference::U64 => nm,
            TypeReference::Float => nm,
            TypeReference::Double => nm,
            TypeReference::String => nm,
            TypeReference::Bytes => nm,
            TypeReference::Enum(_) => format!("{}.ordinal", nm),
            TypeReference::Record(_) => format!("{}.serializeForRust()", nm),
            TypeReference::Optional(_) => format!("{}.serializeForRust()", nm),
            _ => panic!("[TODO: LOWER_KT {:?}]", type_),
        })
    }

    pub fn lift_kt( nm: &dyn fmt::Display, type_: &TypeReference) -> Result<String, askama::Error> {
        let nm = nm.to_string();
        Ok(match type_ {
            TypeReference::Boolean => format!("({} != 0)", nm),
            TypeReference::U32 => nm,
            TypeReference::U64 => nm,
            TypeReference::Float => nm,
            TypeReference::Double => nm,
            TypeReference::String => nm,
            TypeReference::Enum(type_name) => format!("{}.fromOrdinal({})", type_name, nm),
            TypeReference::Record(_)
            | TypeReference::Optional(_) => {
                format!("deserializeFromRust({}) {{ buf -> {} }}", nm, deserialize_item_kt(&"buf", type_)?)
            },
            _ => panic!("[TODO: LIFT_KT {:?}]", type_),
        })
    }

    pub fn deserialize_item_kt(nm: &dyn fmt::Display, type_: &TypeReference) -> Result<String, askama::Error> {
        let nm = nm.to_string();
        Ok(match type_ {
            TypeReference::Optional(t) => {
                // I dont think there's a way to write a generic T?.deserializeItemFromRust() in Kotlin..?
                format!("(if (Boolean.deserializeItemFromRust({})) {{ {} }} else {{ null }})", nm, deserialize_item_kt(&nm, t)?)
            },
            _ => format!("{}.deserializeItemFromRust({})", decl_kt(type_)?, nm)
        })
    }
}